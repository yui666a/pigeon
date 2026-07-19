//! 分類ユースケース（サービス層）。
//!
//! 「メール取得 → サマリ構築 → 修正履歴取得 → LLM実行 → 確信度ゲート →
//! assign / pending / unclassified の振り分け」というユースケース全体を
//! Tauri 非依存（State/AppHandle を受けない）で提供する。
//! commands 層は分類器の構築とこの関数の呼び出しに徹する。
//!
//! 確信度ポリシー（設計: docs/design/2026-04-13-phase2-ai-classification-design.md）
//! はこのモジュールに集約する。

use std::collections::HashMap;
use std::sync::Mutex;

use rusqlite::Connection;

use crate::classifier::{factory, LlmClassifier};
use crate::db::{assignments, mails, projects};
use crate::error::AppError;
use crate::models::classifier::{
    ClassifyAction, ClassifyBatchOutcome, ClassifyResponse, ClassifyResult, MailSummary,
    ProjectSuggestion,
};

/// 自動割り当ての閾値。これ以上の確信度の assign はユーザー確認なしで割り当てる
/// （UI 側は src/utils/classifyConfidence.ts がこの値を鏡写しにしている）。
pub const CONFIDENCE_AUTO_ASSIGN: f64 = 0.7;

/// 要確認の閾値。これ未満の assign は永続化せず未分類のまま扱う。
pub const CONFIDENCE_UNCERTAIN: f64 = 0.4;

/// 分類履歴（訂正ログ）をプロンプトに載せる最大件数。
const CORRECTION_HISTORY_LIMIT: u32 = 20;

/// 「新規案件を作成する」提案の保留キュー（mail_id → 提案内容）。
///
/// エントリはメールの割り当てが確定した時点で必ず除去する（除去漏れは
/// メモリリークと古い提案の残留につながる）。確定経路は以下のすべて:
/// - `approve_new_project` / `reject_classification`（提案自体への応答）
/// - `approve_classification` / `move_mail` / `bulk_move_mails`（手動割り当て）
/// - `classify_one` の高確信度 Assign（再分類で提案が上書きされるケース）
/// - `get_unclassified_threads` のスレッド追従（`auto_follow_threads`）
///
/// なおプロセス内メモリのため、アプリ再起動で提案は消える（揮発性）。
/// 永続化の是非は将来課題。
pub struct PendingClassifications(Mutex<HashMap<String, ClassifyResult>>);

impl Default for PendingClassifications {
    fn default() -> Self {
        Self::new()
    }
}

impl PendingClassifications {
    pub fn new() -> Self {
        Self(Mutex::new(HashMap::new()))
    }

    /// 提案を記録する（同一メールの既存提案は上書き）。
    pub fn insert(&self, mail_id: String, result: ClassifyResult) -> Result<(), AppError> {
        let mut map = self.0.lock().map_err(AppError::lock_err)?;
        map.insert(mail_id, result);
        Ok(())
    }

    /// 提案を除去する（存在しなければ何もしない・冪等）。
    pub fn remove(&self, mail_id: &str) -> Result<(), AppError> {
        let mut map = self.0.lock().map_err(AppError::lock_err)?;
        map.remove(mail_id);
        Ok(())
    }

    /// 提案が保留中かどうか。
    pub fn contains(&self, mail_id: &str) -> Result<bool, AppError> {
        let map = self.0.lock().map_err(AppError::lock_err)?;
        Ok(map.contains_key(mail_id))
    }
}

/// バッチ分類の進行状態（アカウント単位）。Tauri State として管理する。
///
/// 「開始時に未分類スナップショットを取り、create 提案の停止をまたいで
/// インデックスを保持する」ことで、承認/却下済みメールを同一バッチ内で
/// 再分類しない（設計: 2026-07-13-classify-batch-backend-design.md §3.1）。
///
/// `PendingClassifications` と同様プロセス内メモリのため、アプリ再起動で
/// バッチは消える（揮発性）。
#[derive(Default)]
pub struct ClassifyBatches(Mutex<HashMap<String, BatchEntry>>);

struct BatchEntry {
    /// 開始時点の未分類メールIDスナップショット（date DESC）
    queue: Vec<String>,
    /// 次に分類するキュー位置
    index: usize,
    /// ループ実行中（多重 invoke ガード。`SyncLocks` と同じ発想）
    running: bool,
    /// キャンセル要求（ループが次のメール処理前に検知して中断）
    cancelled: bool,
}

impl ClassifyBatches {
    pub fn new() -> Self {
        Self::default()
    }

    /// バッチを開始または再開する。
    /// - 同一アカウントのバッチが実行中なら `None`（多重実行ガード）
    /// - 停止中（承認待ち）のバッチがあれば queue/index を引き継いで再開
    /// - 無ければ `fetch_queue` のスナップショットで新規開始
    fn try_begin(
        &self,
        account_id: &str,
        fetch_queue: impl FnOnce() -> Result<Vec<String>, AppError>,
    ) -> Result<Option<(Vec<String>, usize)>, AppError> {
        let mut map = self.0.lock().map_err(AppError::lock_err)?;
        if let Some(entry) = map.get_mut(account_id) {
            if entry.running {
                return Ok(None);
            }
            entry.running = true;
            entry.cancelled = false;
            return Ok(Some((entry.queue.clone(), entry.index)));
        }
        let queue = fetch_queue()?;
        map.insert(
            account_id.to_string(),
            BatchEntry {
                queue: queue.clone(),
                index: 0,
                running: true,
                cancelled: false,
            },
        );
        Ok(Some((queue, 0)))
    }

    fn is_cancelled(&self, account_id: &str) -> Result<bool, AppError> {
        let map = self.0.lock().map_err(AppError::lock_err)?;
        Ok(map.get(account_id).is_some_and(|e| e.cancelled))
    }

    /// create 提案で停止する（再開位置を保存し running を下ろす）。
    fn pause(&self, account_id: &str, index: usize) -> Result<(), AppError> {
        let mut map = self.0.lock().map_err(AppError::lock_err)?;
        if let Some(entry) = map.get_mut(account_id) {
            entry.index = index;
            entry.running = false;
        }
        Ok(())
    }

    /// バッチを破棄する（完了・キャンセル・エラーの全終端で呼ぶ）。
    fn finish(&self, account_id: &str) -> Result<(), AppError> {
        let mut map = self.0.lock().map_err(AppError::lock_err)?;
        map.remove(account_id);
        Ok(())
    }

    /// キャンセル要求。実行中ならフラグのみ立て（ループが次のメール処理前に
    /// 検知して中断）、停止中（承認待ち）ならバッチを即破棄する。
    pub fn cancel(&self, account_id: &str) -> Result<(), AppError> {
        let mut map = self.0.lock().map_err(AppError::lock_err)?;
        match map.get_mut(account_id) {
            Some(entry) if entry.running => entry.cancelled = true,
            Some(_) => {
                map.remove(account_id);
            }
            None => {}
        }
        Ok(())
    }
}

/// 未分類メールのバッチ分類ユースケース。
///
/// 1 回の呼び出しで「次の停止点（create 提案）または完了/キャンセル」まで進む。
/// - 高確信度の assign は自動割り当てして次のメールへ
/// - create 提案が出たらそこで停止し、pending 登録済みの提案を返す
///   （承認/却下後に再度呼ぶと、次のメールから続行する）
/// - `ClassifyBatches::cancel` が呼ばれていたら次のメール処理前に中断する
///
/// 1 件分の処理は `classify_one` に委譲するため、LLM 呼び出し中に DB ロックを
/// 保持しない方針もそのまま維持される（`ClassifyBatches` のロックも
/// メール間の状態更新時のみ短く取る）。
/// 進捗は `on_progress(処理済み件数, キュー全長, 案件に確定割り当てされた mail_id)`
/// で都度通知する。第3引数は、そのステップで案件へ確定割り当て（Assign）された
/// メールの ID（フロントが未分類一覧から即座に消すために使う）。確信度不足で
/// 未分類に留まった場合や Create 提案の場合は `None`。
pub async fn classify_batch(
    db: &Mutex<Connection>,
    classifier: &dyn LlmClassifier,
    pending: &PendingClassifications,
    batches: &ClassifyBatches,
    account_id: &str,
    on_progress: impl Fn(usize, usize, Option<&str>),
) -> Result<ClassifyBatchOutcome, AppError> {
    let begun = batches.try_begin(account_id, || {
        let conn = db.lock().map_err(AppError::lock_err)?;
        let mails = assignments::get_unclassified_mails(&conn, account_id)?;
        Ok(mails.into_iter().map(|m| m.id).collect())
    })?;
    let Some((queue, start)) = begun else {
        return Ok(ClassifyBatchOutcome::AlreadyRunning);
    };

    let total = queue.len();
    let mut index = start;
    while index < queue.len() {
        if batches.is_cancelled(account_id)? {
            batches.finish(account_id)?;
            return Ok(ClassifyBatchOutcome::Cancelled { done: index, total });
        }
        let response = match classify_one(db, classifier, pending, &queue[index]).await {
            Ok(response) => response,
            Err(e) => {
                // 中断したバッチは破棄する（フロントも状態をリセットする）。
                // 破棄の失敗（ロック毒化）より元エラーの報告を優先する
                let _ = batches.finish(account_id);
                return Err(e);
            }
        };
        index += 1;
        // 案件へ確定割り当て（Assign）されたメールのみ、未分類一覧から消せるよう
        // ID を渡す。確信度不足で Unclassified に落ちた場合や Create 提案は None
        let assigned_mail_id = match response.result.action {
            ClassifyAction::Assign { .. } => Some(response.mail_id.as_str()),
            _ => None,
        };
        on_progress(index, total, assigned_mail_id);
        if matches!(response.result.action, ClassifyAction::Create { .. }) {
            batches.pause(account_id, index)?;
            return Ok(ClassifyBatchOutcome::Paused {
                proposal: response,
                done: index,
                total,
            });
        }
    }

    batches.finish(account_id)?;
    Ok(ClassifyBatchOutcome::Completed { done: index, total })
}

/// メール1通を分類するユースケース一式。
///
/// ロックは「入力スナップショット取得」「結果の永続化」の2回だけ短く取り、
/// LLM 呼び出しはロック外で行う（`project_context::rescan_project` と同じ様式）。
pub async fn classify_one(
    db: &Mutex<Connection>,
    classifier: &dyn LlmClassifier,
    pending: &PendingClassifications,
    mail_id: &str,
) -> Result<ClassifyResponse, AppError> {
    // --- 1. 入力スナップショット取得（ロック内） ---
    let (mail_summary, project_summaries, corrections) = {
        let conn = db.lock().map_err(AppError::lock_err)?;
        let mail = mails::get_mail_by_id(&conn, mail_id)?;
        // クラウドプロバイダ設定時は allow_cloud_context 許可済み案件のみ
        // cached_context を注入する（スペック§5不変条件2）。判定は呼び出し元に
        // 任せず、LLM へ送る入力を組むこの場所で強制する
        let for_cloud = factory::is_cloud_provider_configured(&conn)?;
        let project_summaries =
            projects::build_project_summaries(&conn, &mail.account_id, for_cloud)?;
        let corrections =
            assignments::get_recent_corrections(&conn, &mail.account_id, CORRECTION_HISTORY_LIMIT)?;
        (
            MailSummary::from_mail(&mail),
            project_summaries,
            corrections,
        )
    };

    // --- 2. LLM 実行（ロック外） ---
    classifier.health_check().await?;
    let raw = classifier
        .classify(&mail_summary, &project_summaries, &corrections)
        .await?;

    // --- 3. 確信度ゲート + 永続化（ロック内） ---
    let model_id = classifier.model_id();
    let result = {
        let conn = db.lock().map_err(AppError::lock_err)?;
        apply_result(&conn, pending, mail_id, raw, Some(&model_id))?
    };

    Ok(ClassifyResponse {
        mail_id: mail_id.to_string(),
        result,
    })
}

/// LLM が提案する新規案件名の最大文字数。
pub(crate) const PROPOSED_NAME_MAX_CHARS: usize = 100;
/// LLM が提案する新規案件説明の最大文字数。
pub(crate) const PROPOSED_DESCRIPTION_MAX_CHARS: usize = 300;

/// LLM 出力の案件名/説明から制御文字を除去し長さを制限する
/// （プロンプト再注入・端末/表示汚染対策）。
pub(crate) fn sanitize_proposed_text(value: &str, max_chars: usize) -> String {
    value
        .chars()
        .filter(|c| !c.is_control())
        .take(max_chars)
        .collect::<String>()
        .trim()
        .to_string()
}

/// create 提案の parent_project_id を検証する。存在しない・別アカウントの場合は
/// None（=ルート作成）に落とす。create はユーザー承認制のためエラーにはしない。
pub(crate) fn validate_parent_project(
    conn: &Connection,
    account_id: &str,
    parent_project_id: Option<&str>,
) -> Option<String> {
    let pid = parent_project_id?;
    match projects::get_project(conn, pid) {
        Ok(p) if p.account_id == account_id => Some(p.id),
        _ => None,
    }
}

/// LLM が返した project_id が、当該メールのアカウント配下に実在するか。
/// LLM 出力は信頼できない（幻覚・プロンプトインジェクションで操作可能）ため、
/// 割り当て前にアプリ層で検証する。DB トリガー（trg_mpa_account_check）は
/// アカウント境界のみ担保し、エラーも不透明なため、ここで先に弾く。
fn is_assignable_project(
    conn: &Connection,
    mail_id: &str,
    project_id: &str,
) -> Result<bool, AppError> {
    let mail = mails::get_mail_by_id(conn, mail_id)?;
    match projects::get_project(conn, project_id) {
        Ok(project) => Ok(project.account_id == mail.account_id),
        Err(AppError::ProjectNotFound(_)) => Ok(false),
        Err(e) => Err(e),
    }
}

/// 未分類応答を組み立てる（却下した候補と理由を reason に残す）。
fn unclassified_with_reason(reason: String, confidence: f64) -> ClassifyResult {
    ClassifyResult {
        action: ClassifyAction::Unclassified,
        confidence,
        reason,
    }
}

/// 分類結果を確定・保留・未分類に振り分ける（確信度ポリシーの本体）。
/// 呼び出し元へ返す（＝フロントに見せる）結果を返す。
///
/// - Assign（確信度 >= `CONFIDENCE_UNCERTAIN`）: project_id の帰属を検証の上で
///   割り当てを確定し、過去の分類で残った Create 提案があれば除去する。
///   存在しない/他アカウントの project_id は Unclassified に正規化する
/// - Assign（確信度 < `CONFIDENCE_UNCERTAIN`）: 永続化しない（設計書 §確信度による
///   初期状態: INSERT しない）。DB に存在しない割り当てを「assign」として
///   フロントに見せると承認時に割り当て不在で不整合になるため、応答も
///   Unclassified に正規化する（却下した候補は理由に残す）
/// - Create: 案件名/説明をサニタイズし、確信度によらず提案として保留キューに
///   積む（ユーザー承認待ち）。サニタイズ後に名前が空なら不成立として
///   Unclassified に正規化する
/// - Unclassified: 何も永続化しない
/// AI の判断を1件記録する。
///
/// `persisted` は「確信度ゲートを通過して割り当てが確定する見込みか」を表す。
/// assign かつ確信度が閾値以上のときだけ true。案件の帰属検証で後から
/// Unclassified に正規化されうるが、その場合も「AI は assign を高確信で
/// 選んだ」という事実は分析上残す価値があるため、ここでは true にする。
///
/// 記録の失敗は分類本体を巻き添えにしない。観測のためのログが、分類という
/// 主目的を壊してはならない。
fn log_judgement(
    conn: &Connection,
    mail_id: &str,
    result: &ClassifyResult,
    model: Option<&str>,
) -> Result<(), AppError> {
    let account_id = match mails::get_mail_by_id(conn, mail_id) {
        Ok(mail) => mail.account_id,
        // メールが消えているなら記録先も無い。分類側でエラーになるので黙って諦める
        Err(_) => return Ok(()),
    };

    let (action, project_id, proposed_name, persisted) = match &result.action {
        ClassifyAction::Assign { project_id } => (
            "assign",
            Some(project_id.as_str()),
            None,
            result.confidence >= CONFIDENCE_UNCERTAIN,
        ),
        ClassifyAction::Create { project_name, .. } => {
            ("create", None, Some(project_name.as_str()), false)
        }
        ClassifyAction::Unclassified => ("unclassified", None, None, false),
    };

    // 案件パスは提案時点のスナップショット。案件が後で改名・削除されても
    // 「どこへ入れようとしたか」の意味を保つ
    let project_path = project_id
        .and_then(|id| crate::db::projects::project_path_string(conn, id).ok())
        .filter(|p| !p.is_empty());

    let entry = crate::db::classification_log::ClassificationLogEntry {
        mail_id,
        account_id: &account_id,
        action,
        project_id,
        project_path,
        proposed_name,
        confidence: result.confidence,
        persisted,
        model,
    };
    if let Err(e) = crate::db::classification_log::insert_log(conn, &entry) {
        eprintln!("分類ログの記録に失敗しました（分類自体は継続します）: {e}");
    }
    Ok(())
}

pub fn apply_result(
    conn: &Connection,
    pending: &PendingClassifications,
    mail_id: &str,
    result: ClassifyResult,
    model: Option<&str>,
) -> Result<ClassifyResult, AppError> {
    // AI の判断は永続化の可否によらず記録する。確信度ゲートで破棄された
    // assign や、割り当て行を作らない create / unclassified もここでしか
    // 観測できない（設計: 2026-07-20-classification-observability-design.md）
    log_judgement(conn, mail_id, &result, model)?;
    match result.action {
        ClassifyAction::Assign { ref project_id } if result.confidence >= CONFIDENCE_UNCERTAIN => {
            if !is_assignable_project(conn, mail_id, project_id)? {
                return Ok(unclassified_with_reason(
                    format!(
                        "候補の案件 {project_id} がこのアカウントに存在しないため未分類のままにします。{}",
                        result.reason
                    ),
                    result.confidence,
                ));
            }
            assignments::assign_mail(conn, mail_id, project_id, "ai", Some(result.confidence))?;
            pending.remove(mail_id)?;
            Ok(result)
        }
        ClassifyAction::Assign { ref project_id } => Ok(unclassified_with_reason(
            format!(
                "確信度 {:.2} が閾値 {CONFIDENCE_UNCERTAIN} 未満のため未分類のままにします（候補: {project_id}）。{}",
                result.confidence, result.reason
            ),
            result.confidence,
        )),
        ClassifyAction::Create {
            ref project_name,
            ref description,
            ref parent_project_id,
        } => {
            let name = sanitize_proposed_text(project_name, PROPOSED_NAME_MAX_CHARS);
            if name.is_empty() {
                return Ok(unclassified_with_reason(
                    format!("提案された案件名が不正なため未分類のままにします。{}", result.reason),
                    result.confidence,
                ));
            }
            let mail = mails::get_mail_by_id(conn, mail_id)?;
            let parent_project_id = validate_parent_project(
                conn,
                &mail.account_id,
                parent_project_id.as_deref(),
            );
            let sanitized = ClassifyResult {
                action: ClassifyAction::Create {
                    project_name: name,
                    description: sanitize_proposed_text(
                        description,
                        PROPOSED_DESCRIPTION_MAX_CHARS,
                    ),
                    parent_project_id,
                },
                ..result
            };
            pending.insert(mail_id.to_string(), sanitized.clone())?;
            Ok(sanitized)
        }
        ClassifyAction::Unclassified => Ok(result),
    }
}

/// 選択された複数メールから案件名・説明を1つ提案する。
/// LLM へ送るのは MailSummary（件名・送信者・本文冒頭1000字）のみ。
/// 提案パースに失敗しても空フォールバックで返し、Err にしない
/// （名前が空ならフロントでユーザーが手入力する）。
pub async fn suggest_project_name(
    classifier: &dyn LlmClassifier,
    mails: &[MailSummary],
) -> Result<ProjectSuggestion, AppError> {
    let user_prompt = crate::classifier::prompt::build_suggest_project_prompt(mails);
    let raw = classifier
        .generate_text(
            crate::classifier::prompt::SUGGEST_PROJECT_SYSTEM_PROMPT,
            &user_prompt,
        )
        .await?;
    let parsed = crate::classifier::parse::parse_project_suggestion(&raw);
    Ok(ProjectSuggestion {
        name: sanitize_proposed_text(&parsed.name, PROPOSED_NAME_MAX_CHARS),
        description: sanitize_proposed_text(&parsed.description, PROPOSED_DESCRIPTION_MAX_CHARS),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classifier::TextGenerator;
    use crate::models::project::{CreateProjectRequest, Project};
    use crate::test_helpers::{insert_test_mail, setup_db};
    use async_trait::async_trait;

    /// 固定の分類結果を返すスタブ。応答は実際の JSON パース経路を通す。
    /// health_check の成否も設定できる。
    struct StubLlm {
        result: ClassifyResult,
        healthy: bool,
    }

    impl StubLlm {
        fn returning(result: ClassifyResult) -> Self {
            Self {
                result,
                healthy: true,
            }
        }

        fn unhealthy(result: ClassifyResult) -> Self {
            Self {
                result,
                healthy: false,
            }
        }
    }

    #[async_trait]
    impl TextGenerator for StubLlm {
        async fn generate_text(
            &self,
            _system_prompt: &str,
            _user_prompt: &str,
        ) -> Result<String, AppError> {
            serde_json::to_string(&self.result).map_err(|e| AppError::Classifier(e.to_string()))
        }
    }

    #[async_trait]
    impl LlmClassifier for StubLlm {
        fn model_id(&self) -> String {
            "stub:test".into()
        }

        async fn health_check(&self) -> Result<(), AppError> {
            if self.healthy {
                Ok(())
            } else {
                Err(AppError::OllamaConnection("stub: unhealthy".into()))
            }
        }
    }

    fn assign_result(project_id: &str, confidence: f64) -> ClassifyResult {
        ClassifyResult {
            action: ClassifyAction::Assign {
                project_id: project_id.into(),
            },
            confidence,
            reason: "スタブの理由".into(),
        }
    }

    /// classification_log の行を (action, confidence, persisted) で読む
    fn log_rows(conn: &Connection) -> Vec<(String, f64, i64)> {
        let mut stmt = conn
            .prepare("SELECT action, confidence, persisted FROM classification_log ORDER BY id")
            .unwrap();
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        rows
    }

    /// 確信度ゲートで破棄された assign こそ観測できないと困る。
    /// これが記録されないと「生の確信度分布」が永久に分からない
    /// （設計: 2026-07-20-classification-observability-design.md §1）
    #[test]
    fn test_apply_result_logs_discarded_low_confidence_assign() {
        let conn = setup_db();
        let pending = PendingClassifications::new();
        insert_test_mail(&conn, "m1", "Subject");
        let proj = insert_project(&conn, "Proj");

        apply_result(&conn, &pending, "m1", assign_result(&proj.id, 0.3), None).unwrap();

        let rows = log_rows(&conn);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "assign", "AI が選んだ action を記録する");
        assert!((rows[0].1 - 0.3).abs() < f64::EPSILON, "生の確信度を残す");
        assert_eq!(rows[0].2, 0, "破棄されたので persisted=0");
    }

    #[test]
    fn test_apply_result_logs_persisted_assign() {
        let conn = setup_db();
        let pending = PendingClassifications::new();
        insert_test_mail(&conn, "m1", "Subject");
        let proj = insert_project(&conn, "Proj");

        apply_result(
            &conn,
            &pending,
            "m1",
            assign_result(&proj.id, 0.95),
            Some("gemini_vertex:gemini-3.5-flash"),
        )
        .unwrap();

        let rows = log_rows(&conn);
        assert_eq!(rows, vec![("assign".into(), 0.95, 1)]);

        let (pid, model): (Option<String>, Option<String>) = conn
            .query_row(
                "SELECT project_id, model FROM classification_log",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(pid.as_deref(), Some(proj.id.as_str()));
        assert_eq!(model.as_deref(), Some("gemini_vertex:gemini-3.5-flash"));
    }

    /// create / unclassified は mail_project_assignments に行を作らないため、
    /// ログに残さないと確信度が完全に失われる
    #[test]
    fn test_apply_result_logs_create_and_unclassified() {
        let conn = setup_db();
        let pending = PendingClassifications::new();
        insert_test_mail(&conn, "m1", "Subject");

        apply_result(&conn, &pending, "m1", create_result(), None).unwrap();
        apply_result(&conn, &pending, "m1", unclassified_result(), None).unwrap();

        let rows = log_rows(&conn);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, "create");
        assert_eq!(rows[0].2, 0, "提案は保留キュー止まりで永続化ではない");
        assert_eq!(rows[1].0, "unclassified");
        assert_eq!(rows[1].2, 0);

        let name: Option<String> = conn
            .query_row(
                "SELECT proposed_name FROM classification_log WHERE action = 'create'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(name.as_deref(), Some("Suggested"));
    }

    fn create_result() -> ClassifyResult {
        ClassifyResult {
            action: ClassifyAction::Create {
                project_name: "Suggested".into(),
                description: "desc".into(),
                parent_project_id: None,
            },
            confidence: 0.8,
            reason: "新規案件の提案".into(),
        }
    }

    fn unclassified_result() -> ClassifyResult {
        ClassifyResult {
            action: ClassifyAction::Unclassified,
            confidence: 0.2,
            reason: "判断できない".into(),
        }
    }

    fn insert_project(conn: &Connection, name: &str) -> Project {
        let req = CreateProjectRequest {
            account_id: "acc1".into(),
            name: name.into(),
            description: None,
            color: None,
            parent_id: None,
        };
        projects::insert_project(conn, &req).unwrap()
    }

    fn assigned_mail_ids(db: &Mutex<Connection>, project_id: &str) -> Vec<String> {
        let conn = db.lock().unwrap();
        assignments::get_mails_by_project(&conn, project_id)
            .unwrap()
            .into_iter()
            .map(|m| m.id)
            .collect()
    }

    // --- classify_one: 確信度別の振り分け ---

    #[tokio::test]
    async fn test_classify_one_high_confidence_assign_persists() {
        let db = Mutex::new(setup_db());
        let pending = PendingClassifications::new();
        let proj = {
            let conn = db.lock().unwrap();
            insert_test_mail(&conn, "m1", "見積もりの件");
            insert_project(&conn, "Proj")
        };
        let llm = StubLlm::returning(assign_result(&proj.id, 0.9));

        let res = classify_one(&db, &llm, &pending, "m1").await.unwrap();

        assert_eq!(res.mail_id, "m1");
        assert!(matches!(
            res.result.action,
            ClassifyAction::Assign { ref project_id } if project_id == &proj.id
        ));
        assert_eq!(assigned_mail_ids(&db, &proj.id), vec!["m1".to_string()]);
        assert!(!pending.contains("m1").unwrap());
    }

    #[tokio::test]
    async fn test_classify_one_boundary_confidence_assigns() {
        // 確信度がちょうど閾値のときは割り当てる（>= 判定）
        let db = Mutex::new(setup_db());
        let pending = PendingClassifications::new();
        let proj = {
            let conn = db.lock().unwrap();
            insert_test_mail(&conn, "m1", "Subject");
            insert_project(&conn, "Proj")
        };
        let llm = StubLlm::returning(assign_result(&proj.id, CONFIDENCE_UNCERTAIN));

        let res = classify_one(&db, &llm, &pending, "m1").await.unwrap();

        assert!(matches!(res.result.action, ClassifyAction::Assign { .. }));
        assert_eq!(assigned_mail_ids(&db, &proj.id), vec!["m1".to_string()]);
    }

    #[tokio::test]
    async fn test_classify_one_low_confidence_assign_normalized_to_unclassified() {
        // 設計書（phase2 §確信度による初期状態）: 確信度 < 0.4 の assign は
        // 永続化しない。加えて、DB に存在しない割り当てをフロントに
        // 「assign」として見せない（承認時に割り当て不在で不整合になる）ため、
        // 応答も Unclassified に正規化する。
        let db = Mutex::new(setup_db());
        let pending = PendingClassifications::new();
        let proj = {
            let conn = db.lock().unwrap();
            insert_test_mail(&conn, "m1", "Subject");
            insert_project(&conn, "Proj")
        };
        let llm = StubLlm::returning(assign_result(&proj.id, 0.3));

        let res = classify_one(&db, &llm, &pending, "m1").await.unwrap();

        assert!(
            matches!(res.result.action, ClassifyAction::Unclassified),
            "低確信度の assign は unclassified としてフロントへ返す"
        );
        assert!(
            res.result.reason.contains(&proj.id),
            "却下した候補は理由に残す（デバッグ・透明性のため）"
        );
        assert!(
            assigned_mail_ids(&db, &proj.id).is_empty(),
            "低確信度の assign は永続化しない"
        );
        assert!(
            !pending.contains("m1").unwrap(),
            "pending は新規案件提案（Create）専用。assign は積まない"
        );
    }

    #[tokio::test]
    async fn test_classify_one_create_queues_pending() {
        let db = Mutex::new(setup_db());
        let pending = PendingClassifications::new();
        {
            let conn = db.lock().unwrap();
            insert_test_mail(&conn, "m1", "Subject");
        }
        let llm = StubLlm::returning(create_result());

        let res = classify_one(&db, &llm, &pending, "m1").await.unwrap();

        assert!(matches!(res.result.action, ClassifyAction::Create { .. }));
        assert!(pending.contains("m1").unwrap(), "Create は承認待ちに積む");
    }

    #[tokio::test]
    async fn test_classify_one_unclassified_persists_nothing() {
        let db = Mutex::new(setup_db());
        let pending = PendingClassifications::new();
        let proj = {
            let conn = db.lock().unwrap();
            insert_test_mail(&conn, "m1", "Subject");
            insert_project(&conn, "Proj")
        };
        let llm = StubLlm::returning(unclassified_result());

        let res = classify_one(&db, &llm, &pending, "m1").await.unwrap();

        assert!(matches!(res.result.action, ClassifyAction::Unclassified));
        assert!(assigned_mail_ids(&db, &proj.id).is_empty());
        assert!(!pending.contains("m1").unwrap());
    }

    // --- classify_one: クラウド送信境界（allow_cloud_context の適用） ---

    /// 受け取った user_prompt を記録するスタブ（送信境界のテスト用）。
    struct CapturingLlm {
        result: ClassifyResult,
        prompts: Mutex<Vec<String>>,
    }

    impl CapturingLlm {
        fn returning(result: ClassifyResult) -> Self {
            Self {
                result,
                prompts: Mutex::new(Vec::new()),
            }
        }

        fn captured_prompt(&self) -> String {
            self.prompts.lock().unwrap().join("\n")
        }
    }

    #[async_trait]
    impl TextGenerator for CapturingLlm {
        async fn generate_text(
            &self,
            _system_prompt: &str,
            user_prompt: &str,
        ) -> Result<String, AppError> {
            self.prompts.lock().unwrap().push(user_prompt.to_string());
            serde_json::to_string(&self.result).map_err(|e| AppError::Classifier(e.to_string()))
        }
    }

    #[async_trait]
    impl LlmClassifier for CapturingLlm {
        fn model_id(&self) -> String {
            "stub:test".into()
        }

        async fn health_check(&self) -> Result<(), AppError> {
            Ok(())
        }
    }

    const SECRET_CONTEXT: &str = "社外秘-案件ディレクトリ由来コンテキスト";

    /// メール1通・案件1件（cached_context 付き）と llm_provider 設定を仕込む。
    fn setup_context_boundary(
        db: &Mutex<Connection>,
        provider: &str,
        allow_cloud: bool,
    ) -> Project {
        let conn = db.lock().unwrap();
        insert_test_mail(&conn, "m1", "Subject");
        let proj = insert_project(&conn, "Proj");
        crate::db::project_contexts::upsert_generated(&conn, &proj.id, SECRET_CONTEXT, "h", "i")
            .unwrap();
        crate::db::project_contexts::set_allow_cloud_context(&conn, &proj.id, allow_cloud).unwrap();
        crate::db::settings::set(&conn, "llm_provider", provider).unwrap();
        proj
    }

    #[tokio::test]
    async fn test_classify_one_cloud_provider_excludes_unallowed_context_from_prompt() {
        // クラウドプロバイダ設定では、allow_cloud_context=false の案件の
        // cached_context をプロンプトに含めない（スペック§5不変条件2）
        for provider in ["claude", "claude_vertex", "gemini_vertex"] {
            let db = Mutex::new(setup_db());
            let pending = PendingClassifications::new();
            setup_context_boundary(&db, provider, false);
            let llm = CapturingLlm::returning(unclassified_result());

            classify_one(&db, &llm, &pending, "m1").await.unwrap();

            assert!(
                !llm.captured_prompt().contains(SECRET_CONTEXT),
                "{provider}: 未許可の案件コンテキストがクラウド向けプロンプトに漏れている"
            );
        }
    }

    #[tokio::test]
    async fn test_classify_one_cloud_provider_includes_allowed_context() {
        // allow_cloud_context=true の案件はクラウドでもコンテキストを注入する
        let db = Mutex::new(setup_db());
        let pending = PendingClassifications::new();
        setup_context_boundary(&db, "claude", true);
        let llm = CapturingLlm::returning(unclassified_result());

        classify_one(&db, &llm, &pending, "m1").await.unwrap();

        assert!(llm.captured_prompt().contains(SECRET_CONTEXT));
    }

    #[tokio::test]
    async fn test_classify_one_local_provider_includes_all_context() {
        // ローカル（Ollama）は許可設定に関わらず全コンテキストを注入する（従来挙動）
        let db = Mutex::new(setup_db());
        let pending = PendingClassifications::new();
        setup_context_boundary(&db, "ollama", false);
        let llm = CapturingLlm::returning(unclassified_result());

        classify_one(&db, &llm, &pending, "m1").await.unwrap();

        assert!(llm.captured_prompt().contains(SECRET_CONTEXT));
    }

    #[tokio::test]
    async fn test_classify_one_unknown_mail_returns_not_found() {
        let db = Mutex::new(setup_db());
        let pending = PendingClassifications::new();
        let llm = StubLlm::returning(unclassified_result());

        let res = classify_one(&db, &llm, &pending, "ghost").await;

        assert!(matches!(res, Err(AppError::MailNotFound(_))));
    }

    #[tokio::test]
    async fn test_classify_one_unhealthy_llm_propagates_error_without_persisting() {
        let db = Mutex::new(setup_db());
        let pending = PendingClassifications::new();
        let proj = {
            let conn = db.lock().unwrap();
            insert_test_mail(&conn, "m1", "Subject");
            insert_project(&conn, "Proj")
        };
        let llm = StubLlm::unhealthy(assign_result(&proj.id, 0.9));

        let res = classify_one(&db, &llm, &pending, "m1").await;

        assert!(matches!(res, Err(AppError::OllamaConnection(_))));
        assert!(
            assigned_mail_ids(&db, &proj.id).is_empty(),
            "ヘルスチェック失敗時は何も永続化しない"
        );
    }

    // --- apply_result: project_id の帰属検証（LLM出力は信頼しない） ---

    #[test]
    fn test_apply_result_nonexistent_project_normalized_to_unclassified() {
        // LLM が幻覚で返した存在しない project_id は割り当てず Unclassified に正規化
        let conn = setup_db();
        let pending = PendingClassifications::new();
        insert_test_mail(&conn, "m1", "Subject");

        let res = apply_result(
            &conn,
            &pending,
            "m1",
            assign_result("ghost-project", 0.95),
            None,
        )
        .unwrap();

        assert!(matches!(res.action, ClassifyAction::Unclassified));
        assert!(
            res.reason.contains("ghost-project"),
            "却下した候補を理由に残す"
        );
    }

    #[test]
    fn test_apply_result_cross_account_project_normalized_to_unclassified() {
        // 同一DB内の他アカウントの案件へは割り当てない（DBトリガーの不透明な
        // エラーではなく、アプリ層で Unclassified に正規化する）
        let conn = setup_db();
        let pending = PendingClassifications::new();
        insert_test_mail(&conn, "m1", "Subject");
        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type, provider)
             VALUES ('acc2', 'Other', 'other@example.com', 'imap.example.com', 'smtp.example.com', 'plain', 'other')",
            [],
        )
        .unwrap();
        let req = CreateProjectRequest {
            account_id: "acc2".into(),
            name: "他人の案件".into(),
            description: None,
            color: None,
            parent_id: None,
        };
        let other = projects::insert_project(&conn, &req).unwrap();

        let res =
            apply_result(&conn, &pending, "m1", assign_result(&other.id, 0.95), None).unwrap();

        assert!(matches!(res.action, ClassifyAction::Unclassified));
        assert!(
            assignments::get_mails_by_project(&conn, &other.id)
                .unwrap()
                .is_empty(),
            "他アカウントの案件には割り当てない"
        );
    }

    // --- apply_result: 新規案件提案のサニタイズ ---

    #[test]
    fn test_apply_result_create_strips_control_chars_and_truncates() {
        let conn = setup_db();
        let pending = PendingClassifications::new();
        insert_test_mail(&conn, "m1", "Subject");
        let long_name = format!("注入\u{7}\u{1b}[2J{}", "あ".repeat(300));
        let result = ClassifyResult {
            action: ClassifyAction::Create {
                project_name: long_name,
                description: format!("desc\u{0}{}", "い".repeat(1000)),
                parent_project_id: None,
            },
            confidence: 0.8,
            reason: "r".into(),
        };

        let res = apply_result(&conn, &pending, "m1", result, None).unwrap();

        match res.action {
            ClassifyAction::Create {
                project_name,
                description,
                ..
            } => {
                assert!(
                    !project_name.chars().any(|c| c.is_control()),
                    "制御文字を除去"
                );
                assert!(project_name.chars().count() <= 100, "案件名は100文字まで");
                assert!(project_name.starts_with("注入"));
                assert!(!description.chars().any(|c| c.is_control()));
                assert!(description.chars().count() <= 300, "説明は300文字まで");
            }
            other => panic!("expected Create, got {other:?}"),
        }
    }

    #[test]
    fn test_apply_result_create_empty_name_normalized_to_unclassified() {
        // 制御文字だけの案件名は提案として成立しない
        let conn = setup_db();
        let pending = PendingClassifications::new();
        insert_test_mail(&conn, "m1", "Subject");
        let result = ClassifyResult {
            action: ClassifyAction::Create {
                project_name: "\u{7}\u{1b} ".into(),
                description: "d".into(),
                parent_project_id: None,
            },
            confidence: 0.8,
            reason: "r".into(),
        };

        let res = apply_result(&conn, &pending, "m1", result, None).unwrap();

        assert!(matches!(res.action, ClassifyAction::Unclassified));
        assert!(!pending.contains("m1").unwrap(), "不成立の提案は積まない");
    }

    // --- validate_parent_project / apply_result: create の parent_project_id 検証 ---

    #[test]
    fn test_validate_parent_project_falls_back_to_root_on_hallucination() {
        // 存在しない/別アカウントの parent_project_id は None に落とす
        let conn = setup_db();
        crate::db::projects::insert_project_with_id(
            &conn,
            "root",
            "acc1",
            "ツアー",
            None,
            None,
            None,
        )
        .unwrap();
        assert_eq!(
            validate_parent_project(&conn, "acc1", Some("root")),
            Some("root".to_string()),
            "実在する同一アカウントの親は通す"
        );
        assert_eq!(
            validate_parent_project(&conn, "acc1", Some("ghost")),
            None,
            "存在しない親はルート作成に落とす"
        );
        assert_eq!(validate_parent_project(&conn, "acc1", None), None);
    }

    #[test]
    fn test_apply_result_create_keeps_valid_parent_project_id() {
        let conn = setup_db();
        let pending = PendingClassifications::new();
        insert_test_mail(&conn, "m1", "Subject");
        let parent = insert_project(&conn, "ツアー");
        let result = ClassifyResult {
            action: ClassifyAction::Create {
                project_name: "音響".into(),
                description: "d".into(),
                parent_project_id: Some(parent.id.clone()),
            },
            confidence: 0.8,
            reason: "r".into(),
        };

        let res = apply_result(&conn, &pending, "m1", result, None).unwrap();

        match res.action {
            ClassifyAction::Create {
                parent_project_id, ..
            } => assert_eq!(parent_project_id, Some(parent.id)),
            other => panic!("expected Create, got {other:?}"),
        }
    }

    #[test]
    fn test_apply_result_create_drops_hallucinated_parent_project_id() {
        // 存在しない parent_project_id はエラーにせず None（ルート作成）に落とす
        let conn = setup_db();
        let pending = PendingClassifications::new();
        insert_test_mail(&conn, "m1", "Subject");
        let result = ClassifyResult {
            action: ClassifyAction::Create {
                project_name: "音響".into(),
                description: "d".into(),
                parent_project_id: Some("ghost-project".into()),
            },
            confidence: 0.8,
            reason: "r".into(),
        };

        let res = apply_result(&conn, &pending, "m1", result, None).unwrap();

        match res.action {
            ClassifyAction::Create {
                parent_project_id, ..
            } => assert_eq!(
                parent_project_id, None,
                "存在しない親はルート作成として続行する（エラーにしない）"
            ),
            other => panic!("expected Create, got {other:?}"),
        }
    }

    // --- apply_result: 確信度ゲートの単体挙動 ---

    #[test]
    fn test_apply_result_assign_removes_stale_pending() {
        let conn = setup_db();
        let pending = PendingClassifications::new();
        insert_test_mail(&conn, "m1", "Subject");
        let proj = insert_project(&conn, "Proj");
        // 以前の分類で Create 提案が残っている状態
        pending.insert("m1".into(), create_result()).unwrap();

        apply_result(
            &conn,
            &pending,
            "m1",
            assign_result(&proj.id, CONFIDENCE_UNCERTAIN),
            None,
        )
        .unwrap();

        assert!(
            !pending.contains("m1").unwrap(),
            "高確信度の割り当てで古い提案は除去される"
        );
        let assigned = assignments::get_mails_by_project(&conn, &proj.id).unwrap();
        assert_eq!(assigned.len(), 1);
    }

    #[test]
    fn test_apply_result_create_queues_pending() {
        let conn = setup_db();
        let pending = PendingClassifications::new();
        insert_test_mail(&conn, "m1", "Subject");

        apply_result(&conn, &pending, "m1", create_result(), None).unwrap();

        assert!(pending.contains("m1").unwrap());
    }

    // --- PendingClassifications ---

    #[test]
    fn test_pending_classifications_insert_and_remove() {
        let pending = PendingClassifications::new();

        pending.insert("mail-1".into(), create_result()).unwrap();
        pending.insert("mail-2".into(), create_result()).unwrap();

        assert!(pending.contains("mail-1").unwrap());
        assert!(pending.contains("mail-2").unwrap());

        pending.remove("mail-1").unwrap();
        assert!(!pending.contains("mail-1").unwrap());
        assert!(pending.contains("mail-2").unwrap());
    }

    // --- classify_batch: バッチ分類のワークフロー ---

    /// メールごとに異なる結果を順番に返すスタブ（バッチのワークフローテスト用）。
    struct SeqLlm {
        results: Mutex<Vec<ClassifyResult>>,
    }

    impl SeqLlm {
        fn new(results: Vec<ClassifyResult>) -> Self {
            Self {
                results: Mutex::new(results),
            }
        }

        fn remaining(&self) -> usize {
            self.results.lock().unwrap().len()
        }
    }

    #[async_trait]
    impl TextGenerator for SeqLlm {
        async fn generate_text(
            &self,
            _system_prompt: &str,
            _user_prompt: &str,
        ) -> Result<String, AppError> {
            let result = self.results.lock().unwrap().remove(0);
            serde_json::to_string(&result).map_err(|e| AppError::Classifier(e.to_string()))
        }
    }

    #[async_trait]
    impl LlmClassifier for SeqLlm {
        fn model_id(&self) -> String {
            "stub:test".into()
        }

        async fn health_check(&self) -> Result<(), AppError> {
            Ok(())
        }
    }

    /// date DESC でキューに載る順（m1 → m2 → m3）に3通の未分類メールを入れる。
    fn insert_batch_mails(conn: &Connection) {
        for (id, date) in [
            ("m1", "2026-07-13T12:00:00"),
            ("m2", "2026-07-13T11:00:00"),
            ("m3", "2026-07-13T10:00:00"),
        ] {
            let mail =
                crate::test_helpers::make_mail(id, &format!("<{id}@ex.com>"), "Subject", date);
            crate::db::mails::insert_mail(conn, &mail).unwrap();
        }
    }

    #[tokio::test]
    async fn test_classify_batch_all_assign_completes_with_progress() {
        let db = Mutex::new(setup_db());
        let pending = PendingClassifications::new();
        let batches = ClassifyBatches::new();
        let proj = {
            let conn = db.lock().unwrap();
            insert_batch_mails(&conn);
            insert_project(&conn, "Proj")
        };
        let llm = SeqLlm::new(vec![assign_result(&proj.id, 0.9); 3]);
        let progress: Mutex<Vec<(usize, usize)>> = Mutex::new(Vec::new());

        let outcome = classify_batch(&db, &llm, &pending, &batches, "acc1", |c, t, _| {
            progress.lock().unwrap().push((c, t));
        })
        .await
        .unwrap();

        assert!(matches!(
            outcome,
            ClassifyBatchOutcome::Completed { done: 3, total: 3 }
        ));
        assert_eq!(assigned_mail_ids(&db, &proj.id).len(), 3);
        assert_eq!(*progress.lock().unwrap(), vec![(1, 3), (2, 3), (3, 3)]);
    }

    #[tokio::test]
    async fn test_classify_batch_reports_assigned_mail_id_on_confident_assign() {
        // 高確信度の Assign では、そのステップで確定した mail_id が
        // on_progress の第3引数として通知される（フロントの逐次非表示用）
        let db = Mutex::new(setup_db());
        let pending = PendingClassifications::new();
        let batches = ClassifyBatches::new();
        let proj = {
            let conn = db.lock().unwrap();
            insert_batch_mails(&conn);
            insert_project(&conn, "Proj")
        };
        let llm = SeqLlm::new(vec![assign_result(&proj.id, 0.9); 3]);
        let assigned: Mutex<Vec<Option<String>>> = Mutex::new(Vec::new());

        classify_batch(&db, &llm, &pending, &batches, "acc1", |_, _, id| {
            assigned.lock().unwrap().push(id.map(str::to_string));
        })
        .await
        .unwrap();

        // 3件すべて確定割り当てされ、それぞれの mail_id が通知される
        let ids = assigned.lock().unwrap();
        assert_eq!(ids.len(), 3);
        assert!(ids.iter().all(|id| id.is_some()));
    }

    #[tokio::test]
    async fn test_classify_batch_reports_none_when_confidence_too_low() {
        // 確信度が閾値未満の Assign は永続化されず未分類に留まるため、
        // 確定 mail_id は通知されない（None）
        let db = Mutex::new(setup_db());
        let pending = PendingClassifications::new();
        let batches = ClassifyBatches::new();
        let proj = {
            let conn = db.lock().unwrap();
            insert_batch_mails(&conn);
            insert_project(&conn, "Proj")
        };
        // CONFIDENCE_UNCERTAIN 未満 → apply_result が Unclassified に正規化する
        let low = CONFIDENCE_UNCERTAIN - 0.1;
        let llm = SeqLlm::new(vec![assign_result(&proj.id, low); 3]);
        let assigned: Mutex<Vec<Option<String>>> = Mutex::new(Vec::new());

        classify_batch(&db, &llm, &pending, &batches, "acc1", |_, _, id| {
            assigned.lock().unwrap().push(id.map(str::to_string));
        })
        .await
        .unwrap();

        let ids = assigned.lock().unwrap();
        assert_eq!(ids.len(), 3);
        assert!(ids.iter().all(|id| id.is_none()));
    }

    #[tokio::test]
    async fn test_classify_batch_pauses_on_create_and_resumes_from_next_mail() {
        let db = Mutex::new(setup_db());
        let pending = PendingClassifications::new();
        let batches = ClassifyBatches::new();
        let proj = {
            let conn = db.lock().unwrap();
            insert_batch_mails(&conn);
            insert_project(&conn, "Proj")
        };
        let llm = SeqLlm::new(vec![
            create_result(),
            assign_result(&proj.id, 0.9),
            assign_result(&proj.id, 0.9),
        ]);

        // 1回目: 先頭の m1 が create → そこで停止し提案を返す
        let outcome = classify_batch(&db, &llm, &pending, &batches, "acc1", |_, _, _| {})
            .await
            .unwrap();

        match outcome {
            ClassifyBatchOutcome::Paused {
                proposal,
                done,
                total,
            } => {
                assert_eq!(proposal.mail_id, "m1");
                assert!(matches!(
                    proposal.result.action,
                    ClassifyAction::Create { .. }
                ));
                assert_eq!((done, total), (1, 3));
            }
            other => panic!("expected Paused, got {other:?}"),
        }
        assert!(pending.contains("m1").unwrap(), "提案は承認待ちに積まれる");
        assert_eq!(llm.remaining(), 2, "create で停止し後続はまだ分類しない");

        // 2回目（承認/却下後の再開に相当）: m1 は再分類せず m2 から続行して完了
        let progress: Mutex<Vec<(usize, usize)>> = Mutex::new(Vec::new());
        let outcome = classify_batch(&db, &llm, &pending, &batches, "acc1", |c, t, _| {
            progress.lock().unwrap().push((c, t));
        })
        .await
        .unwrap();

        assert!(matches!(
            outcome,
            ClassifyBatchOutcome::Completed { done: 3, total: 3 }
        ));
        assert_eq!(llm.remaining(), 0);
        assert_eq!(
            assigned_mail_ids(&db, &proj.id).len(),
            2,
            "m1（承認待ちのまま）は再分類されない"
        );
        assert_eq!(
            *progress.lock().unwrap(),
            vec![(2, 3), (3, 3)],
            "進捗は元の total のまま続きから通知される"
        );
    }

    #[tokio::test]
    async fn test_classify_batch_cancel_stops_before_next_mail() {
        let db = Mutex::new(setup_db());
        let pending = PendingClassifications::new();
        let batches = ClassifyBatches::new();
        let proj = {
            let conn = db.lock().unwrap();
            insert_batch_mails(&conn);
            insert_project(&conn, "Proj")
        };
        let llm = SeqLlm::new(vec![assign_result(&proj.id, 0.9); 3]);

        // 1件目の処理直後にユーザーがキャンセルした状況を再現
        let outcome = classify_batch(&db, &llm, &pending, &batches, "acc1", |_, _, _| {
            batches.cancel("acc1").unwrap();
        })
        .await
        .unwrap();

        assert!(matches!(
            outcome,
            ClassifyBatchOutcome::Cancelled { done: 1, total: 3 }
        ));
        assert_eq!(llm.remaining(), 2, "キャンセル後は分類しない");

        // バッチは破棄済み: 次回は新しいスナップショット（残り2件）で開始する
        let outcome = classify_batch(&db, &llm, &pending, &batches, "acc1", |_, _, _| {})
            .await
            .unwrap();
        assert!(matches!(
            outcome,
            ClassifyBatchOutcome::Completed { done: 2, total: 2 }
        ));
    }

    #[tokio::test]
    async fn test_classify_batch_error_discards_batch() {
        let db = Mutex::new(setup_db());
        let pending = PendingClassifications::new();
        let batches = ClassifyBatches::new();
        let proj = {
            let conn = db.lock().unwrap();
            insert_batch_mails(&conn);
            insert_project(&conn, "Proj")
        };

        let bad = StubLlm::unhealthy(assign_result(&proj.id, 0.9));
        let res = classify_batch(&db, &bad, &pending, &batches, "acc1", |_, _, _| {}).await;
        assert!(matches!(res, Err(AppError::OllamaConnection(_))));

        // エラーでバッチは破棄され、次回は新規スナップショットで最初から
        let good = SeqLlm::new(vec![assign_result(&proj.id, 0.9); 3]);
        let outcome = classify_batch(&db, &good, &pending, &batches, "acc1", |_, _, _| {})
            .await
            .unwrap();
        assert!(matches!(
            outcome,
            ClassifyBatchOutcome::Completed { done: 3, total: 3 }
        ));
    }

    #[tokio::test]
    async fn test_classify_batch_already_running_guard() {
        let db = Mutex::new(setup_db());
        let pending = PendingClassifications::new();
        let batches = ClassifyBatches::new();
        {
            let conn = db.lock().unwrap();
            insert_batch_mails(&conn);
        }
        // 実行中相当の状態を作る（try_begin 済みで pause も finish もしていない）
        batches.try_begin("acc1", || Ok(vec!["m1".into()])).unwrap();

        let llm = SeqLlm::new(vec![]);
        let outcome = classify_batch(&db, &llm, &pending, &batches, "acc1", |_, _, _| {})
            .await
            .unwrap();

        assert!(matches!(outcome, ClassifyBatchOutcome::AlreadyRunning));
        assert_eq!(llm.remaining(), 0, "多重実行では何も分類しない");
    }

    // --- ClassifyBatches: 状態遷移 ---

    #[test]
    fn test_classify_batches_rejects_second_begin_while_running() {
        let batches = ClassifyBatches::new();
        let first = batches
            .try_begin("acc1", || Ok(vec!["m1".into(), "m2".into()]))
            .unwrap();
        assert!(first.is_some());

        let second = batches.try_begin("acc1", || Ok(vec![])).unwrap();
        assert!(second.is_none(), "実行中の多重開始は拒否する");
    }

    #[test]
    fn test_classify_batches_pause_and_resume_keeps_queue_and_index() {
        let batches = ClassifyBatches::new();
        let queue: Vec<String> = vec!["m1".into(), "m2".into(), "m3".into()];
        batches.try_begin("acc1", || Ok(queue.clone())).unwrap();
        batches.pause("acc1", 2).unwrap();

        let resumed = batches
            .try_begin("acc1", || panic!("再開時はスナップショットを取り直さない"))
            .unwrap();

        assert_eq!(resumed, Some((queue, 2)));
    }

    #[test]
    fn test_classify_batches_cancel_while_paused_discards_batch() {
        let batches = ClassifyBatches::new();
        batches.try_begin("acc1", || Ok(vec!["m1".into()])).unwrap();
        batches.pause("acc1", 1).unwrap();

        batches.cancel("acc1").unwrap();

        // 破棄済みなので次回は新規開始（スナップショットを取り直す）
        let begun = batches.try_begin("acc1", || Ok(vec!["m2".into()])).unwrap();
        assert_eq!(begun, Some((vec!["m2".into()], 0)));
    }

    #[test]
    fn test_classify_batches_finish_discards_batch() {
        let batches = ClassifyBatches::new();
        batches.try_begin("acc1", || Ok(vec!["m1".into()])).unwrap();
        batches.pause("acc1", 1).unwrap();
        batches.finish("acc1").unwrap();

        let begun = batches.try_begin("acc1", || Ok(vec!["m2".into()])).unwrap();
        assert_eq!(begun, Some((vec!["m2".into()], 0)));
    }

    // --- suggest_project_name: 提案生成 ---

    /// 固定テキストを返す `LlmClassifier` スタブ（提案テスト用）。
    struct TextStubLlm(String);

    #[async_trait]
    impl TextGenerator for TextStubLlm {
        async fn generate_text(
            &self,
            _system_prompt: &str,
            _user_prompt: &str,
        ) -> Result<String, AppError> {
            Ok(self.0.clone())
        }
    }

    #[async_trait]
    impl LlmClassifier for TextStubLlm {
        fn model_id(&self) -> String {
            "stub:test".into()
        }

        async fn health_check(&self) -> Result<(), AppError> {
            Ok(())
        }
    }

    fn one_mail() -> Vec<MailSummary> {
        vec![MailSummary {
            subject: "s".into(),
            from_addr: "f".into(),
            date: "d".into(),
            body_preview: "b".into(),
        }]
    }

    #[tokio::test]
    async fn test_suggest_project_name_parses_and_sanitizes() {
        let llm = TextStubLlm(r#"{"name": "在庫管理", "description": "説明"}"#.into());
        let s = super::suggest_project_name(&llm, &one_mail())
            .await
            .unwrap();
        assert_eq!(s.name, "在庫管理");
        assert_eq!(s.description, "説明");
    }

    #[tokio::test]
    async fn test_suggest_project_name_unparseable_returns_empty() {
        let llm = TextStubLlm("no json".into());
        let s = super::suggest_project_name(&llm, &one_mail())
            .await
            .unwrap();
        assert_eq!(s.name, "");
        assert_eq!(s.description, "");
    }
}
