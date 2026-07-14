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

use crate::classifier::LlmClassifier;
use crate::db::{assignments, mails, projects};
use crate::error::AppError;
use crate::models::classifier::{
    ClassifyAction, ClassifyBatchOutcome, ClassifyResponse, ClassifyResult, MailSummary,
};

/// 自動割り当ての閾値。これ以上の確信度の assign はユーザー確認なしで割り当てる
/// （UI 側のバッジ配色もこの値を鏡写しにしている: ClassifyResultBadge.tsx）。
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
        let project_summaries = projects::build_project_summaries(&conn, &mail.account_id, false)?;
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
    let result = {
        let conn = db.lock().map_err(AppError::lock_err)?;
        apply_result(&conn, pending, mail_id, raw)?
    };

    Ok(ClassifyResponse {
        mail_id: mail_id.to_string(),
        result,
    })
}

/// 分類結果を確定・保留・未分類に振り分ける（確信度ポリシーの本体）。
/// 呼び出し元へ返す（＝フロントに見せる）結果を返す。
///
/// - Assign（確信度 >= `CONFIDENCE_UNCERTAIN`）: 割り当てを確定し、過去の分類で
///   残った Create 提案があれば除去する
/// - Assign（確信度 < `CONFIDENCE_UNCERTAIN`）: 永続化しない（設計書 §確信度による
///   初期状態: INSERT しない）。DB に存在しない割り当てを「assign」として
///   フロントに見せると承認時に割り当て不在で不整合になるため、応答も
///   Unclassified に正規化する（却下した候補は理由に残す）
/// - Create: 確信度によらず提案として保留キューに積む（ユーザー承認待ち）
/// - Unclassified: 何も永続化しない
pub fn apply_result(
    conn: &Connection,
    pending: &PendingClassifications,
    mail_id: &str,
    result: ClassifyResult,
) -> Result<ClassifyResult, AppError> {
    match result.action {
        ClassifyAction::Assign { ref project_id } if result.confidence >= CONFIDENCE_UNCERTAIN => {
            assignments::assign_mail(conn, mail_id, project_id, "ai", Some(result.confidence))?;
            pending.remove(mail_id)?;
            Ok(result)
        }
        ClassifyAction::Assign { ref project_id } => Ok(ClassifyResult {
            reason: format!(
                "確信度 {:.2} が閾値 {CONFIDENCE_UNCERTAIN} 未満のため未分類のままにします（候補: {project_id}）。{}",
                result.confidence, result.reason
            ),
            action: ClassifyAction::Unclassified,
            confidence: result.confidence,
        }),
        ClassifyAction::Create { .. } => {
            pending.insert(mail_id.to_string(), result.clone())?;
            Ok(result)
        }
        ClassifyAction::Unclassified => Ok(result),
    }
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

    fn create_result() -> ClassifyResult {
        ClassifyResult {
            action: ClassifyAction::Create {
                project_name: "Suggested".into(),
                description: "desc".into(),
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

        apply_result(&conn, &pending, "m1", create_result()).unwrap();

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
}
