use tauri::{AppHandle, Emitter, State};

use crate::classifier::factory::build_classifier;
use crate::classifier::service::{self, ClassifyBatches, PendingClassifications};
use crate::context::Ctx;
use crate::db::{assignments, mails, projects};
use crate::error::AppError;
use crate::models::classifier::{ClassifyBatchOutcome, ClassifyResponse, ProjectSuggestion};
use crate::models::project::Project;
use crate::state::{DbState, SecureStoreState, SyncLocks};

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// Classify a single mail by ID and persist the result.
///
/// ユースケースの本体（サマリ構築 → LLM実行 → 確信度ゲート → 振り分け）は
/// `classifier::service::classify_one` にあり、ここでは分類器の構築と
/// サービス呼び出しのみを行う。
#[tauri::command]
pub async fn classify_mail(
    db: State<'_, DbState>,
    pending: State<'_, PendingClassifications>,
    batches: State<'_, ClassifyBatches>,
    sync_locks: State<'_, SyncLocks>,
    secure_store: State<'_, SecureStoreState>,
    mail_id: String,
) -> Result<ClassifyResponse, AppError> {
    let ctx = Ctx::new(&db, &secure_store, &pending, &batches, &sync_locks);
    let classifier = ctx.with_conn(|conn| build_classifier(conn, ctx.secure_store()?))?;
    service::classify_one(&db.0, classifier.as_ref(), ctx.pending(), &mail_id).await
}

/// classify-progress イベントの payload
#[derive(Clone, serde::Serialize)]
struct ClassifyProgressEvent {
    account_id: String,
    current: usize,
    total: usize,
    /// このステップで案件へ確定割り当てされたメールの ID（あれば）。
    /// フロントが未分類一覧から即座に消すために使う。
    assigned_mail_id: Option<String>,
}

/// 未分類メールのバッチ分類を開始/再開する。
///
/// 1 invoke で「次の停止点（create 提案）または完了/キャンセル」まで進む。
/// ループの本体は `classifier::service::classify_batch` にあり、ここでは
/// 分類器の構築と classify-progress イベントの emit のみを行う
/// （`sync_account` / `sync_service` と同じ分業）。
#[tauri::command]
pub async fn classify_batch(
    app: AppHandle,
    db: State<'_, DbState>,
    pending: State<'_, PendingClassifications>,
    batches: State<'_, ClassifyBatches>,
    secure_store: State<'_, SecureStoreState>,
    account_id: String,
) -> Result<ClassifyBatchOutcome, AppError> {
    let classifier = db.with_conn(|conn| build_classifier(conn, &secure_store.0))?;
    service::classify_batch(
        &db.0,
        classifier.as_ref(),
        &pending,
        &batches,
        &account_id,
        |current, total, assigned_mail_id| {
            // 進捗はベストエフォート（emit 失敗で分類は止めない）
            let _ = app.emit(
                "classify-progress",
                ClassifyProgressEvent {
                    account_id: account_id.clone(),
                    current,
                    total,
                    assigned_mail_id: assigned_mail_id.map(str::to_string),
                },
            );
        },
    )
    .await
}

/// 実行中/承認待ちのバッチ分類を中止する。
/// 実行中ならフラグを立てて次のメール処理前に中断させ、
/// 承認待ちで停止中ならバッチを即破棄する。
#[tauri::command]
pub fn cancel_classification(
    batches: State<ClassifyBatches>,
    account_id: String,
) -> Result<(), AppError> {
    batches.cancel(&account_id)
}

/// Approve an AI classification (user confirms the assigned project).
#[tauri::command]
pub fn approve_classification(
    db: State<DbState>,
    pending: State<PendingClassifications>,
    mail_id: String,
    project_id: String,
) -> Result<(), AppError> {
    db.with_conn(|conn| approve_classification_inner(conn, &pending, &mail_id, &project_id))
}

fn approve_classification_inner(
    conn: &rusqlite::Connection,
    pending: &PendingClassifications,
    mail_id: &str,
    project_id: &str,
) -> Result<(), AppError> {
    assignments::approve_classification(conn, mail_id, project_id)?;
    // 割り当てが確定したので、残っている提案があれば除去する
    pending.remove(mail_id)
}

/// Approve a "create new project" suggestion: creates the project and assigns the mail.
#[tauri::command]
pub fn approve_new_project(
    db: State<DbState>,
    pending: State<PendingClassifications>,
    mail_id: String,
    project_name: String,
    description: Option<String>,
    parent_project_id: Option<String>,
) -> Result<Project, AppError> {
    let project = db.with_conn_mut(|conn| {
        // Load mail to get account_id
        let mail = mails::get_mail_by_id(conn, &mail_id)?;

        let tx = conn.transaction()?;

        let project = projects::insert_project_with_id(
            &tx,
            &uuid::Uuid::new_v4().to_string(),
            &mail.account_id,
            &project_name,
            description.as_deref(),
            None,
            parent_project_id.as_deref(),
        )?;

        assignments::assign_mail(&tx, &mail_id, &project.id, "user", Some(1.0))?;

        tx.commit()?;
        Ok(project)
    })?;

    // Remove from pending map
    pending.remove(&mail_id)?;
    Ok(project)
}

/// Reject an AI classification (remove from pending or delete assignment).
#[tauri::command]
pub fn reject_classification(
    db: State<DbState>,
    pending: State<PendingClassifications>,
    mail_id: String,
) -> Result<(), AppError> {
    // Remove from pending map if present
    pending.remove(&mail_id)?;

    // Also remove from DB assignments if present
    let result = db.with_conn(|conn| assignments::reject_classification(conn, &mail_id));
    match result {
        Ok(()) => Ok(()),
        // MailNotFound means there was no assignment -- that's fine after removing from pending
        Err(AppError::MailNotFound(_)) => Ok(()),
        Err(e) => Err(e),
    }
}

/// 未分類メールをスレッド単位で返す（未分類一覧のスレッド表示用）。
/// 分類の実体はメール単位のまま（スレッドのD&Dは全メールIDを渡す）。
///
/// 取得の前にスレッド追従の自動分類（`auto_follow_threads`）を行う。
/// 同一スレッドの既存メールが単一の案件に割り当て済みなら、後から届いた返信等の
/// 未分類メールをその案件へ自動追従させる。一覧を開くたびに再計算する
/// （設計: docs/archive/specs/2026-07-13-thread-follow-classify-design.md）
///
/// 一覧は必ず上限を持ち、切り出しはスレッド単位で行う（ADR 0006 決定5）。
#[tauri::command]
pub fn get_unclassified_threads(
    db: State<DbState>,
    pending: State<PendingClassifications>,
    account_id: String,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<crate::models::mail::ThreadPage, AppError> {
    let limit = limit
        .unwrap_or(crate::commands::mail_commands::DEFAULT_THREAD_PAGE_SIZE)
        .min(crate::commands::mail_commands::MAX_THREAD_PAGE_SIZE);
    let offset = offset.unwrap_or(0);
    db.with_conn(|conn| get_unclassified_threads_inner(conn, &pending, &account_id, limit, offset))
}

fn get_unclassified_threads_inner(
    conn: &rusqlite::Connection,
    pending: &PendingClassifications,
    account_id: &str,
    limit: usize,
    offset: usize,
) -> Result<crate::models::mail::ThreadPage, AppError> {
    let followed = assignments::auto_follow_threads(conn, account_id)?;
    // スレッド追従で割り当てが確定したメールの提案は不要になる
    for mail_id in &followed {
        pending.remove(mail_id)?;
    }
    // 1段目: 本文を読まない軽量メタでスレッドを構成し、窓をスレッド境界で切る
    let metas = assignments::get_unclassified_thread_metas(conn, account_id)?;
    let groups = crate::db::mails::group_mail_ids_into_threads(&metas);
    let total = groups.len();
    let page_ids: Vec<String> = groups
        .into_iter()
        .skip(offset)
        .take(limit)
        .flatten()
        .collect();
    let has_more = total > offset.saturating_add(limit);

    // 2段目: 窓に入ったスレッドのメールだけを本文込みで読む
    let mails = assignments::get_unclassified_mails_by_ids(conn, &page_ids)?;
    Ok(crate::models::mail::ThreadPage {
        threads: crate::db::mails::build_threads(&mails),
        has_more,
    })
}

/// メール1件を案件へ割り当てる本体。`bulk_move_mails` から1件ずつ再利用される。
/// 単件移動の入口は bulk に一本化しているため、この関数を直接呼ぶ command はない
/// （設計判断: docs/adr/0004-ai-native-dispatch-architecture.md D12）。
pub(crate) fn move_mail_inner(
    conn: &rusqlite::Connection,
    pending: &PendingClassifications,
    mail_id: &str,
    project_id: &str,
) -> Result<(), AppError> {
    assignments::move_mail_to_project(conn, mail_id, project_id)?;
    // 割り当てが確定したので、残っている提案があれば除去する
    pending.remove(mail_id)
}

/// 選択された複数メールから、新規案件の名前・説明を LLM に提案させる。
/// 案件作成・メール移動はフロント側で既存の create_project / bulk_move_mails
/// を合成して行うため、この command は「提案の取得」だけを担う。
#[tauri::command]
pub async fn suggest_project_from_mails(
    db: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    mail_ids: Vec<String>,
) -> Result<ProjectSuggestion, AppError> {
    // --- メール要約の取得（ロック内） ---
    let summaries: Vec<crate::models::classifier::MailSummary> = db.with_conn(|conn| {
        let mut out = Vec::with_capacity(mail_ids.len());
        for id in &mail_ids {
            let mail = mails::get_mail_by_id(conn, id)?;
            out.push(crate::models::classifier::MailSummary::from_mail(&mail));
        }
        Ok(out)
    })?;

    // --- LLM 実行（ロック外） ---
    let classifier = db.with_conn(|conn| build_classifier(conn, &secure_store.0))?;
    classifier.health_check().await?;
    service::suggest_project_name(classifier.as_ref(), &summaries).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::classifier::{ClassifyAction, ClassifyResult};
    use crate::models::project::CreateProjectRequest;
    use crate::test_helpers::{insert_test_mail, setup_db};

    // --- get_mail_by_id (now in db::mails) ---

    #[test]
    fn test_get_mail_by_id_success() {
        let conn = setup_db();
        insert_test_mail(&conn, "m1", "Test Subject");
        let mail = mails::get_mail_by_id(&conn, "m1").unwrap();
        assert_eq!(mail.id, "m1");
        assert_eq!(mail.subject, "Test Subject");
    }

    #[test]
    fn test_get_mail_by_id_not_found() {
        let conn = setup_db();
        let result = mails::get_mail_by_id(&conn, "nonexistent");
        assert!(result.is_err());
    }

    // --- build_project_summaries ---

    #[test]
    fn test_build_project_summaries_empty() {
        let conn = setup_db();
        let summaries = projects::build_project_summaries(&conn, "acc1", false).unwrap();
        assert!(summaries.is_empty());
    }

    #[test]
    fn test_build_project_summaries_with_projects() {
        let conn = setup_db();
        let req = CreateProjectRequest {
            account_id: "acc1".into(),
            name: "Project Alpha".into(),
            description: Some("First project".into()),
            color: None,
            parent_id: None,
        };
        projects::insert_project(&conn, &req).unwrap();
        let summaries = projects::build_project_summaries(&conn, "acc1", false).unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].name, "Project Alpha");
    }

    // --- approve_new_project flow ---

    #[test]
    fn test_approve_new_project_creates_project_and_assigns_mail() {
        let mut conn = setup_db();
        insert_test_mail(&conn, "m1", "New Deal");

        // Simulate what approve_new_project does
        let mail = mails::get_mail_by_id(&conn, "m1").unwrap();
        let tx = conn.transaction().unwrap();
        let req = CreateProjectRequest {
            account_id: mail.account_id.clone(),
            name: "New Project".into(),
            description: Some("Auto-created".into()),
            color: None,
            parent_id: None,
        };
        let project = projects::insert_project(&tx, &req).unwrap();
        assignments::assign_mail(&tx, "m1", &project.id, "user", Some(1.0)).unwrap();
        tx.commit().unwrap();

        // Verify project was created
        let projs = projects::list_projects(&conn, "acc1").unwrap();
        assert_eq!(projs.len(), 1);
        assert_eq!(projs[0].name, "New Project");

        // Verify mail was assigned
        let assigned_mails = assignments::get_mails_by_project(&conn, &projs[0].id).unwrap();
        assert_eq!(assigned_mails.len(), 1);
        assert_eq!(assigned_mails[0].id, "m1");
    }

    #[test]
    fn test_approve_new_project_transaction_rollback_on_error() {
        let mut conn = setup_db();
        // Don't insert mail — assign_mail will still succeed (no FK on mail_id in some schemas)
        // but we can test that transaction rolls back if we manually drop it
        let tx = conn.transaction().unwrap();
        let req = CreateProjectRequest {
            account_id: "acc1".into(),
            name: "Will Rollback".into(),
            description: None,
            color: None,
            parent_id: None,
        };
        let _project = projects::insert_project(&tx, &req).unwrap();
        // Drop tx without committing — should rollback
        drop(tx);

        let projs = projects::list_projects(&conn, "acc1").unwrap();
        assert!(projs.is_empty(), "Transaction should have been rolled back");
    }

    // --- reject_classification flow ---

    #[test]
    fn test_reject_removes_from_pending_map() {
        let pending = PendingClassifications::new();
        let result = ClassifyResult {
            action: ClassifyAction::Create {
                project_name: "Test".into(),
                description: "desc".into(),
                parent_project_id: None,
            },
            confidence: 0.8,
            reason: "test".into(),
        };
        pending.insert("m1".into(), result).unwrap();

        // Remove from pending
        pending.remove("m1").unwrap();
        assert!(!pending.contains("m1").unwrap());
    }

    // --- get_unclassified_mails flow ---

    #[test]
    fn test_get_unclassified_mails_returns_unassigned() {
        let conn = setup_db();
        insert_test_mail(&conn, "m1", "Unassigned Mail");
        insert_test_mail(&conn, "m2", "Also Unassigned");

        let unclassified = assignments::get_unclassified_mails(&conn, "acc1").unwrap();
        assert_eq!(unclassified.len(), 2);
    }

    #[test]
    fn test_get_unclassified_mails_excludes_sent_and_archive() {
        // 自分の送信済み・アーカイブ済みは分類対象にしない（INBOXのみ）
        let conn = setup_db();
        insert_test_mail(&conn, "m1", "Inbox Mail");
        let mut sent = crate::test_helpers::make_mail(
            "m2",
            "<sent@pigeon.local>",
            "Re: Inbox Mail",
            "2026-07-12T10:00:00",
        );
        sent.folder = "Sent".into();
        crate::db::mails::insert_mail(&conn, &sent).unwrap();
        let mut archived = crate::test_helpers::make_mail(
            "m3",
            "<archived@ex.com>",
            "Old Mail",
            "2026-07-11T10:00:00",
        );
        archived.folder = "Archive".into();
        crate::db::mails::insert_mail(&conn, &archived).unwrap();

        let unclassified = assignments::get_unclassified_mails(&conn, "acc1").unwrap();
        assert_eq!(unclassified.len(), 1);
        assert_eq!(unclassified[0].id, "m1");
    }

    #[test]
    fn test_get_unclassified_threads_auto_follows_before_listing() {
        // 既に割り当て済みの m1 に対する返信 m2 は、一覧取得時に自動追従され
        // 未分類一覧から消えること
        let conn = setup_db();
        let m1 =
            crate::test_helpers::make_mail("m1", "<m1@ex.com>", "Re: Test", "2026-07-12T10:00:00");
        let mut m2 =
            crate::test_helpers::make_mail("m2", "<m2@ex.com>", "Re: Test", "2026-07-12T11:00:00");
        m2.in_reply_to = Some("<m1@ex.com>".into());
        crate::db::mails::insert_mail(&conn, &m1).unwrap();
        crate::db::mails::insert_mail(&conn, &m2).unwrap();

        let req = CreateProjectRequest {
            account_id: "acc1".into(),
            name: "Proj".into(),
            description: None,
            color: None,
            parent_id: None,
        };
        let proj = projects::insert_project(&conn, &req).unwrap();
        assignments::assign_mail(&conn, "m1", &proj.id, "user", Some(1.0)).unwrap();

        let mails = assignments::get_unclassified_mails(&conn, "acc1").unwrap();
        let threads_before = crate::db::mails::build_threads(&mails);
        assert_eq!(threads_before.len(), 1, "m2は追従前は未分類一覧に見える");

        // get_unclassified_threads相当の処理（auto_follow_threads → 再取得）を直接呼ぶ
        assignments::auto_follow_threads(&conn, "acc1").unwrap();
        let mails_after = assignments::get_unclassified_mails(&conn, "acc1").unwrap();
        assert!(mails_after.is_empty(), "追従によりm2も未分類一覧から消える");
    }

    #[test]
    fn test_get_unclassified_threads_groups_replies() {
        // 返信の連鎖（References）が1スレッドにまとまること
        let conn = setup_db();
        let m1 =
            crate::test_helpers::make_mail("m1", "<t1@ex.com>", "Re: Test", "2026-07-12T10:00:00");
        let mut m2 =
            crate::test_helpers::make_mail("m2", "<t3@ex.com>", "Re: Test", "2026-07-12T11:00:00");
        // 中間のメール（自分の返信 <t2> は Sent にあり未分類一覧には無い）を
        // 飛び越えて References で繋がるケース
        m2.references = Some("<t1@ex.com> <t2@pigeon.local>".into());
        crate::db::mails::insert_mail(&conn, &m1).unwrap();
        crate::db::mails::insert_mail(&conn, &m2).unwrap();
        insert_test_mail(&conn, "m3", "Unrelated");

        let mails = assignments::get_unclassified_mails(&conn, "acc1").unwrap();
        let threads = crate::db::mails::build_threads(&mails);
        assert_eq!(threads.len(), 2);
        let re_test = threads.iter().find(|t| t.mail_count == 2).unwrap();
        assert_eq!(re_test.mails.len(), 2);
    }

    #[test]
    fn test_get_unclassified_mails_excludes_assigned() {
        let conn = setup_db();
        insert_test_mail(&conn, "m1", "Assigned Mail");
        insert_test_mail(&conn, "m2", "Unassigned Mail");

        let req = CreateProjectRequest {
            account_id: "acc1".into(),
            name: "Proj".into(),
            description: None,
            color: None,
            parent_id: None,
        };
        let proj = projects::insert_project(&conn, &req).unwrap();
        assignments::assign_mail(&conn, "m1", &proj.id, "ai", Some(0.9)).unwrap();

        let unclassified = assignments::get_unclassified_mails(&conn, "acc1").unwrap();
        assert_eq!(unclassified.len(), 1);
        assert_eq!(unclassified[0].id, "m2");
    }

    // --- move_mail flow ---

    #[test]
    fn test_move_mail_between_projects() {
        let conn = setup_db();
        insert_test_mail(&conn, "m1", "Moving Mail");

        let req1 = CreateProjectRequest {
            account_id: "acc1".into(),
            name: "Project A".into(),
            description: None,
            color: None,
            parent_id: None,
        };
        let req2 = CreateProjectRequest {
            account_id: "acc1".into(),
            name: "Project B".into(),
            description: None,
            color: None,
            parent_id: None,
        };
        let proj_a = projects::insert_project(&conn, &req1).unwrap();
        let proj_b = projects::insert_project(&conn, &req2).unwrap();

        assignments::assign_mail(&conn, "m1", &proj_a.id, "ai", Some(0.9)).unwrap();
        assignments::move_mail_to_project(&conn, "m1", &proj_b.id).unwrap();

        let mails_a = assignments::get_mails_by_project(&conn, &proj_a.id).unwrap();
        let mails_b = assignments::get_mails_by_project(&conn, &proj_b.id).unwrap();
        assert!(mails_a.is_empty());
        assert_eq!(mails_b.len(), 1);
        assert_eq!(mails_b[0].id, "m1");
    }

    // --- pending リーク防止: 割り当てが確定する全経路で提案が除去される ---

    fn pending_create_result() -> ClassifyResult {
        ClassifyResult {
            action: ClassifyAction::Create {
                project_name: "Suggested".into(),
                description: "desc".into(),
                parent_project_id: None,
            },
            confidence: 0.8,
            reason: "test".into(),
        }
    }

    fn insert_project_for(conn: &rusqlite::Connection, name: &str) -> Project {
        let req = CreateProjectRequest {
            account_id: "acc1".into(),
            name: name.into(),
            description: None,
            color: None,
            parent_id: None,
        };
        projects::insert_project(conn, &req).unwrap()
    }

    #[test]
    fn test_move_mail_removes_pending_entry() {
        let conn = setup_db();
        let pending = PendingClassifications::new();
        insert_test_mail(&conn, "m1", "Subject");
        let proj = insert_project_for(&conn, "Proj");
        pending
            .insert("m1".into(), pending_create_result())
            .unwrap();

        move_mail_inner(&conn, &pending, "m1", &proj.id).unwrap();

        assert!(
            !pending.contains("m1").unwrap(),
            "手動割り当てで確定したら提案は残らない"
        );
    }

    #[test]
    fn test_move_mail_failure_keeps_pending_entry() {
        let conn = setup_db();
        let pending = PendingClassifications::new();
        // メールがDBに無いので move は失敗する
        pending
            .insert("ghost".into(), pending_create_result())
            .unwrap();

        let result = move_mail_inner(&conn, &pending, "ghost", "proj-x");

        assert!(result.is_err());
        assert!(
            pending.contains("ghost").unwrap(),
            "割り当てが確定していないときは提案を保持する"
        );
    }

    #[test]
    fn test_approve_classification_removes_pending_entry() {
        let conn = setup_db();
        let pending = PendingClassifications::new();
        insert_test_mail(&conn, "m1", "Subject");
        let proj = insert_project_for(&conn, "Proj");
        assignments::assign_mail(&conn, "m1", &proj.id, "ai", Some(0.9)).unwrap();
        pending
            .insert("m1".into(), pending_create_result())
            .unwrap();

        approve_classification_inner(&conn, &pending, "m1", &proj.id).unwrap();

        assert!(
            !pending.contains("m1").unwrap(),
            "承認で確定したら提案は残らない"
        );
    }

    #[test]
    fn test_get_unclassified_threads_inner_removes_followed_pending() {
        // スレッド追従で割り当てが確定したメールの Create 提案も除去される
        let conn = setup_db();
        let pending = PendingClassifications::new();
        let m1 =
            crate::test_helpers::make_mail("m1", "<m1@ex.com>", "Re: Test", "2026-07-12T10:00:00");
        let mut m2 =
            crate::test_helpers::make_mail("m2", "<m2@ex.com>", "Re: Test", "2026-07-12T11:00:00");
        m2.in_reply_to = Some("<m1@ex.com>".into());
        crate::db::mails::insert_mail(&conn, &m1).unwrap();
        crate::db::mails::insert_mail(&conn, &m2).unwrap();
        let proj = insert_project_for(&conn, "Proj");
        assignments::assign_mail(&conn, "m1", &proj.id, "user", Some(1.0)).unwrap();
        pending
            .insert("m2".into(), pending_create_result())
            .unwrap();

        let page = get_unclassified_threads_inner(&conn, &pending, "acc1", 200, 0).unwrap();

        assert!(
            page.threads.is_empty(),
            "m2 は追従割り当てされ一覧から消える"
        );
        assert!(!page.has_more);
        assert!(
            !pending.contains("m2").unwrap(),
            "追従で確定したメールの提案も除去される"
        );
    }

    /// 未分類一覧もスレッド単位で切る（ADR 0006 決定5）。スレッドが
    /// 分断されないこと・ページ間で重複しないことを見る
    #[test]
    fn test_get_unclassified_threads_pages_by_thread() {
        let conn = setup_db();
        let pending = PendingClassifications::default();

        // 独立3スレッド + 2通スレッド1件（計4スレッド、5通）
        for i in 0..3 {
            let m = crate::test_helpers::make_mail(
                &format!("s{i}"),
                &format!("<s{i}@ex.com>"),
                &format!("単独 {i}"),
                &format!("2026-07-12T0{i}:00:00"),
            );
            crate::db::mails::insert_mail(&conn, &m).unwrap();
        }
        let root =
            crate::test_helpers::make_mail("r1", "<r1@ex.com>", "会話", "2026-07-12T10:00:00");
        let mut reply =
            crate::test_helpers::make_mail("r2", "<r2@ex.com>", "Re: 会話", "2026-07-12T11:00:00");
        reply.in_reply_to = Some("<r1@ex.com>".into());
        crate::db::mails::insert_mail(&conn, &root).unwrap();
        crate::db::mails::insert_mail(&conn, &reply).unwrap();

        let first = get_unclassified_threads_inner(&conn, &pending, "acc1", 1, 0).unwrap();
        assert_eq!(first.threads.len(), 1);
        assert!(first.has_more);
        assert_eq!(
            first.threads[0].mails.len(),
            2,
            "最新スレッドは2通揃って返る（メール単位LIMITなら1通に欠ける）"
        );

        let rest = get_unclassified_threads_inner(&conn, &pending, "acc1", 10, 1).unwrap();
        assert_eq!(rest.threads.len(), 3, "残りの単独スレッド");
        assert!(!rest.has_more);

        // ページ間でメールが重複しない
        let mut ids: Vec<String> = first
            .threads
            .iter()
            .chain(rest.threads.iter())
            .flat_map(|t| t.mails.iter().map(|m| m.id.clone()))
            .collect();
        let total = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), total);
        assert_eq!(total, 5, "全メールを網羅する");
    }
}
