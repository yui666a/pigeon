//! 分類ユースケース（サービス層）。
//!
//! 「メール取得 → サマリ構築 → 修正履歴取得 → LLM実行 → 確信度ゲート →
//! assign / pending / unclassified の振り分け」というユースケース全体を
//! Tauri 非依存（State/AppHandle を受けない）で提供する。
//! commands 層は分類器の構築とこの関数の呼び出しに徹する。
//!
//! 確信度ポリシー（設計: docs/superpowers/specs/2026-04-13-phase2-ai-classification-design.md）
//! はこのモジュールに集約する。

use std::collections::HashMap;
use std::sync::Mutex;

use rusqlite::Connection;

use crate::classifier::LlmClassifier;
use crate::db::{assignments, mails, projects};
use crate::error::AppError;
use crate::models::classifier::{ClassifyAction, ClassifyResponse, ClassifyResult, MailSummary};

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
///   初期状態: INSERT しない）
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
        // 低確信度の Assign は永続化しない
        ClassifyAction::Assign { .. } => Ok(result),
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
}
