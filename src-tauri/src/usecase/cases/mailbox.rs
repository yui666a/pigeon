//! 削除・アーカイブ系の use case（Phase 4-3 Sensitive 抽出）。
//! Risk はプラン依存: mail_policy の実効プラン（サーバー反映の有無）を
//! DB から引いて決める（ADR 0004 D3）。本体は commands 側の
//! delete_mail_inner / archive_mail_inner に委譲する。

use serde::Deserialize;

use crate::commands::bulk_commands::BulkResult;
use crate::commands::mail_commands::{
    archive_mail_inner, delete_mail_inner, resolve_imap_credentials, unarchive_mail_inner,
};
use crate::commands::mail_policy::{plan_archive, plan_delete, ArchivePlan, DeletePlan};
use crate::context::Ctx;
use crate::db::{accounts, mails, settings};
use crate::error::AppError;
use crate::models::mail::{ThreadPage, UnreadCounts};
use crate::usecase::{Registry, Risk, UseCase};

/// 対象メールの削除プランから実効 Risk を求める。
/// サーバー削除を伴えば Sensitive、ローカル行のみなら Reversible。
fn delete_risk(ctx: &Ctx, mail_id: &str) -> Result<Risk, AppError> {
    let mail = ctx.with_conn(|conn| mails::get_mail_by_id(conn, mail_id))?;
    Ok(match plan_delete(&mail.folder) {
        DeletePlan::Server => Risk::Sensitive,
        DeletePlan::LocalOnly => Risk::Reversible,
    })
}

/// 対象メールのアーカイブプランから実効 Risk を求める。
/// サーバー反映（DeleteOnly / CopyThenDelete）を伴えば Sensitive。
fn archive_risk(ctx: &Ctx, account_id: &str, mail_id: &str) -> Result<Risk, AppError> {
    let (account, mail, archive_folder) = ctx.with_conn(|conn| {
        Ok((
            accounts::get_account(conn, account_id)?,
            mails::get_mail_by_id(conn, mail_id)?,
            settings::get_or_default(conn, "archive_folder", "Archive")?,
        ))
    })?;
    Ok(
        match plan_archive(&account.provider, &mail.folder, &archive_folder) {
            ArchivePlan::LocalOnly => Risk::Reversible,
            ArchivePlan::DeleteOnly | ArchivePlan::CopyThenDelete(_) => Risk::Sensitive,
        },
    )
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct DeleteMailInput {
    pub account_id: String,
    pub mail_id: String,
}

/// メール削除（単体）。
pub struct DeleteMailUseCase;

#[async_trait::async_trait]
impl UseCase for DeleteMailUseCase {
    type Input = DeleteMailInput;
    type Output = ();

    fn name(&self) -> &'static str {
        "delete_mail"
    }

    fn risk(&self, input: &Self::Input, ctx: &Ctx) -> Result<Risk, AppError> {
        delete_risk(ctx, &input.mail_id)
    }

    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        let account = ctx.with_conn(|conn| accounts::get_account(conn, &input.account_id))?;
        // 資格情報は遅延解決（Sent の削除はローカルのみで IMAP 認証が不要）
        let creds = tokio::sync::OnceCell::new();
        delete_mail_inner(
            ctx.db(),
            ctx.secure_store()?,
            &account,
            &creds,
            &input.mail_id,
        )
        .await
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct ArchiveMailInput {
    pub account_id: String,
    pub mail_id: String,
}

/// メールアーカイブ（単体）。
pub struct ArchiveMailUseCase;

#[async_trait::async_trait]
impl UseCase for ArchiveMailUseCase {
    type Input = ArchiveMailInput;
    type Output = ();

    fn name(&self) -> &'static str {
        "archive_mail"
    }

    fn risk(&self, input: &Self::Input, ctx: &Ctx) -> Result<Risk, AppError> {
        archive_risk(ctx, &input.account_id, &input.mail_id)
    }

    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        let (account, archive_folder) = ctx.with_conn(|conn| {
            Ok((
                accounts::get_account(conn, &input.account_id)?,
                settings::get_or_default(conn, "archive_folder", "Archive")?,
            ))
        })?;
        let creds = tokio::sync::OnceCell::new();
        archive_mail_inner(
            ctx.db(),
            ctx.secure_store()?,
            &account,
            &archive_folder,
            &creds,
            &input.mail_id,
        )
        .await
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct UnarchiveMailInput {
    pub account_id: String,
    pub mail_id: String,
}

/// アーカイブ解除（ローカルのみ。v1 制限はコマンド側ドキュメント参照）。
pub struct UnarchiveMailUseCase;

#[async_trait::async_trait]
impl UseCase for UnarchiveMailUseCase {
    type Input = UnarchiveMailInput;
    type Output = ();

    fn name(&self) -> &'static str {
        "unarchive_mail"
    }

    fn risk(&self, _input: &Self::Input, _ctx: &Ctx) -> Result<Risk, AppError> {
        Ok(Risk::Reversible)
    }

    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        ctx.with_conn(|conn| unarchive_mail_inner(conn, &input.account_id, &input.mail_id))
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct BulkDeleteMailsInput {
    pub account_id: String,
    pub mail_ids: Vec<String>,
}

/// 一括削除。1件でもサーバー削除を伴えば Sensitive。
pub struct BulkDeleteMailsUseCase;

#[async_trait::async_trait]
impl UseCase for BulkDeleteMailsUseCase {
    type Input = BulkDeleteMailsInput;
    type Output = BulkResult;

    fn name(&self) -> &'static str {
        "bulk_delete_mails"
    }

    fn risk(&self, input: &Self::Input, ctx: &Ctx) -> Result<Risk, AppError> {
        for mail_id in &input.mail_ids {
            // 存在しないメールは run 側で部分失敗として扱うため、ここでは無視する
            if matches!(delete_risk(ctx, mail_id), Ok(Risk::Sensitive)) {
                return Ok(Risk::Sensitive);
            }
        }
        Ok(Risk::Reversible)
    }

    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        let account = ctx.with_conn(|conn| accounts::get_account(conn, &input.account_id))?;
        let secure_store = ctx.secure_store()?;
        // 資格情報は開始時に一度だけ解決して全メールで使い回す
        let creds = tokio::sync::OnceCell::new_with(Some(
            resolve_imap_credentials(&account, secure_store).await?,
        ));

        let mut result = BulkResult::new();
        for mail_id in input.mail_ids {
            let outcome =
                delete_mail_inner(ctx.db(), secure_store, &account, &creds, &mail_id).await;
            result.push(mail_id, outcome);
        }
        Ok(result)
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct BulkArchiveMailsInput {
    pub account_id: String,
    pub mail_ids: Vec<String>,
}

/// 一括アーカイブ。1件でもサーバー反映を伴えば Sensitive。
pub struct BulkArchiveMailsUseCase;

#[async_trait::async_trait]
impl UseCase for BulkArchiveMailsUseCase {
    type Input = BulkArchiveMailsInput;
    type Output = BulkResult;

    fn name(&self) -> &'static str {
        "bulk_archive_mails"
    }

    fn risk(&self, input: &Self::Input, ctx: &Ctx) -> Result<Risk, AppError> {
        for mail_id in &input.mail_ids {
            if matches!(
                archive_risk(ctx, &input.account_id, mail_id),
                Ok(Risk::Sensitive)
            ) {
                return Ok(Risk::Sensitive);
            }
        }
        Ok(Risk::Reversible)
    }

    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        let (account, archive_folder) = ctx.with_conn(|conn| {
            Ok((
                accounts::get_account(conn, &input.account_id)?,
                settings::get_or_default(conn, "archive_folder", "Archive")?,
            ))
        })?;
        let secure_store = ctx.secure_store()?;
        let creds = tokio::sync::OnceCell::new_with(Some(
            resolve_imap_credentials(&account, secure_store).await?,
        ));

        let mut result = BulkResult::new();
        for mail_id in input.mail_ids {
            let outcome = archive_mail_inner(
                ctx.db(),
                secure_store,
                &account,
                &archive_folder,
                &creds,
                &mail_id,
            )
            .await;
            result.push(mail_id, outcome);
        }
        Ok(result)
    }
}

/// 一覧取得の既定ページサイズ（スレッド単位）。
/// 呼び出し側が `limit` を省略したときに適用される。
/// フロントの `src/constants/paging.ts` の `THREAD_PAGE_SIZE` と揃える。
pub const DEFAULT_THREAD_PAGE_SIZE: usize = 200;

/// 1リクエストで返せるスレッド数の上限。巨大な `limit` を投げられても
/// 全件転送に退化しないための歯止め（ADR 0006 決定5）。
///
/// usecase 層で掛けるのは、GUI だけでなく CLI / MCP から任意の `limit` が
/// 渡りうるため。バスを通る全 driver に同じ上限が効く。
pub const MAX_THREAD_PAGE_SIZE: usize = 500;

/// `limit` / `offset` の省略値と上限を解決する。3つの一覧 use case が共用する。
pub fn resolve_page_window(limit: Option<usize>, offset: Option<usize>) -> (usize, usize) {
    (
        limit
            .unwrap_or(DEFAULT_THREAD_PAGE_SIZE)
            .min(MAX_THREAD_PAGE_SIZE),
        offset.unwrap_or(0),
    )
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct GetThreadsInput {
    /// 対象アカウントの ID。
    pub account_id: String,
    /// 対象フォルダ名（例: "INBOX"）。
    pub folder: String,
    /// 返すスレッドの最大件数。省略時は 200。
    ///
    /// 上限はメールではなくスレッドに掛かる。窓に入ったスレッドは
    /// 常にメールが揃った状態で返るため、`limit: 1` でも 5 通の
    /// スレッドは 5 通すべて含んで返る。件数の見積もりには使えない。
    pub limit: Option<usize>,
    /// 読み飛ばすスレッド数。省略時は 0。
    ///
    /// 続きを取るには前回の `limit` + `offset` を渡す。
    /// `has_more` が false になるまで繰り返せば全件を走査できる。
    /// 総件数は返さない（毎回の全走査を避けるため）。
    pub offset: Option<usize>,
}

/// フォルダ内メールをスレッド化して1ページ分返す（読み取り）。
pub struct GetThreadsUseCase;

#[async_trait::async_trait]
impl UseCase for GetThreadsUseCase {
    type Input = GetThreadsInput;
    type Output = ThreadPage;

    fn name(&self) -> &'static str {
        "get_threads"
    }

    fn risk(&self, _input: &Self::Input, _ctx: &Ctx) -> Result<Risk, AppError> {
        Ok(Risk::Read)
    }

    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        let (limit, offset) = resolve_page_window(input.limit, input.offset);
        ctx.with_conn(|conn| {
            mails::get_thread_page_by_account(conn, &input.account_id, &input.folder, limit, offset)
        })
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct GetThreadsByProjectInput {
    /// 対象案件の ID。子案件のメールも集約して返す。
    pub project_id: String,
    /// 返すスレッドの最大件数。省略時は 200。
    /// 上限はスレッド単位に掛かり、窓に入ったスレッドはメールが揃って返る。
    pub limit: Option<usize>,
    /// 読み飛ばすスレッド数。省略時は 0。
    /// 続きを取るには前回の `limit` + `offset` を渡し、`has_more` が
    /// false になるまで繰り返す。
    pub offset: Option<usize>,
}

/// 案件に紐づくスレッド一覧を1ページ分返す（読み取り）。
pub struct GetThreadsByProjectUseCase;

#[async_trait::async_trait]
impl UseCase for GetThreadsByProjectUseCase {
    type Input = GetThreadsByProjectInput;
    type Output = ThreadPage;

    fn name(&self) -> &'static str {
        "get_threads_by_project"
    }

    fn risk(&self, _input: &Self::Input, _ctx: &Ctx) -> Result<Risk, AppError> {
        Ok(Risk::Read)
    }

    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        let (limit, offset) = resolve_page_window(input.limit, input.offset);
        ctx.with_conn(|conn| {
            mails::get_thread_page_by_project(conn, &input.project_id, limit, offset)
        })
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct GetUnreadCountsInput {
    pub account_id: String,
}

/// 案件毎 + 未分類の未読件数（読み取り）。
pub struct GetUnreadCountsUseCase;

#[async_trait::async_trait]
impl UseCase for GetUnreadCountsUseCase {
    type Input = GetUnreadCountsInput;
    type Output = UnreadCounts;

    fn name(&self) -> &'static str {
        "get_unread_counts"
    }

    fn risk(&self, _input: &Self::Input, _ctx: &Ctx) -> Result<Risk, AppError> {
        Ok(Risk::Read)
    }

    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        ctx.with_conn(|conn| mails::get_unread_counts(conn, &input.account_id))
    }
}

pub fn register_mailbox_cases(registry: &mut Registry) {
    registry.register(GetThreadsUseCase);
    registry.register(GetThreadsByProjectUseCase);
    registry.register(GetUnreadCountsUseCase);
    registry.register(DeleteMailUseCase);
    registry.register(ArchiveMailUseCase);
    registry.register(UnarchiveMailUseCase);
    registry.register(BulkDeleteMailsUseCase);
    registry.register(BulkArchiveMailsUseCase);
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::classifier::service::{ClassifyBatches, PendingClassifications};
    use crate::state::{DbState, SyncLocks};
    use crate::test_helpers::{make_mail, setup_db};

    fn build_states() -> (DbState, PendingClassifications, ClassifyBatches, SyncLocks) {
        (
            DbState(Mutex::new(setup_db())),
            PendingClassifications::new(),
            ClassifyBatches::new(),
            SyncLocks::new(),
        )
    }

    /// テスト用の SecureStore（InMemory）。local-only 分岐では参照が取れれば
    /// 十分で、実体には触れない。実 Stronghold のスナップショット I/O を回避する。
    fn test_secure_store() -> crate::secure_store::SecureStore {
        crate::secure_store::SecureStore::in_memory()
    }

    fn insert_mail_in_folder(db: &DbState, id: &str, folder: &str) {
        let mut mail = make_mail(
            id,
            &format!("<{id}@ex.com>"),
            "Subject",
            "2026-07-15T10:00:00",
        );
        mail.folder = folder.into();
        db.with_conn(|conn| crate::db::mails::insert_mail(conn, &mail))
            .unwrap();
    }

    #[test]
    fn test_delete_risk_is_plan_dependent() {
        let (db, pending, batches, locks) = build_states();
        insert_mail_in_folder(&db, "inbox1", "INBOX");
        insert_mail_in_folder(&db, "sent1", "Sent");
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks);

        // INBOX はサーバー削除を伴う → Sensitive
        let input = DeleteMailInput {
            account_id: "acc1".into(),
            mail_id: "inbox1".into(),
        };
        assert_eq!(
            DeleteMailUseCase.risk(&input, &ctx).unwrap(),
            Risk::Sensitive
        );

        // Sent はローカル行の削除のみ → Reversible
        let input = DeleteMailInput {
            account_id: "acc1".into(),
            mail_id: "sent1".into(),
        };
        assert_eq!(
            DeleteMailUseCase.risk(&input, &ctx).unwrap(),
            Risk::Reversible
        );
    }

    #[test]
    fn test_archive_risk_is_plan_dependent() {
        let (db, pending, batches, locks) = build_states();
        insert_mail_in_folder(&db, "inbox1", "INBOX");
        insert_mail_in_folder(&db, "sent1", "Sent");
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks);

        let input = ArchiveMailInput {
            account_id: "acc1".into(),
            mail_id: "inbox1".into(),
        };
        assert_eq!(
            ArchiveMailUseCase.risk(&input, &ctx).unwrap(),
            Risk::Sensitive
        );

        let input = ArchiveMailInput {
            account_id: "acc1".into(),
            mail_id: "sent1".into(),
        };
        assert_eq!(
            ArchiveMailUseCase.risk(&input, &ctx).unwrap(),
            Risk::Reversible
        );
    }

    #[test]
    fn test_bulk_delete_risk_escalates_if_any_server_side() {
        let (db, pending, batches, locks) = build_states();
        insert_mail_in_folder(&db, "inbox1", "INBOX");
        insert_mail_in_folder(&db, "sent1", "Sent");
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks);

        // Sent のみ → Reversible
        let input = BulkDeleteMailsInput {
            account_id: "acc1".into(),
            mail_ids: vec!["sent1".into()],
        };
        assert_eq!(
            BulkDeleteMailsUseCase.risk(&input, &ctx).unwrap(),
            Risk::Reversible
        );

        // INBOX を1件でも含む → Sensitive
        let input = BulkDeleteMailsInput {
            account_id: "acc1".into(),
            mail_ids: vec!["sent1".into(), "inbox1".into()],
        };
        assert_eq!(
            BulkDeleteMailsUseCase.risk(&input, &ctx).unwrap(),
            Risk::Sensitive
        );
    }

    #[tokio::test]
    async fn test_delete_sent_mail_removes_local_row() {
        let (db, pending, batches, locks) = build_states();
        insert_mail_in_folder(&db, "sent1", "Sent");
        let store = test_secure_store();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks).with_secure_store(&store);

        DeleteMailUseCase
            .run(
                DeleteMailInput {
                    account_id: "acc1".into(),
                    mail_id: "sent1".into(),
                },
                &ctx,
            )
            .await
            .expect("delete of Sent mail is local-only and should succeed");

        let result = db.with_conn(|conn| crate::db::mails::get_mail_by_id(conn, "sent1"));
        assert!(matches!(result, Err(AppError::MailNotFound(_))));
    }

    #[tokio::test]
    async fn test_archive_sent_mail_updates_folder_locally() {
        let (db, pending, batches, locks) = build_states();
        insert_mail_in_folder(&db, "sent1", "Sent");
        let store = test_secure_store();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks).with_secure_store(&store);

        ArchiveMailUseCase
            .run(
                ArchiveMailInput {
                    account_id: "acc1".into(),
                    mail_id: "sent1".into(),
                },
                &ctx,
            )
            .await
            .expect("archive of Sent mail is local-only and should succeed");

        let mail = db
            .with_conn(|conn| crate::db::mails::get_mail_by_id(conn, "sent1"))
            .unwrap();
        assert_eq!(mail.folder, "Archive");
    }

    #[tokio::test]
    async fn test_get_threads_returns_empty_for_unknown_account() {
        let (db, pending, batches, sync_locks) = build_states();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &sync_locks);
        let uc = GetThreadsUseCase;
        let input = GetThreadsInput {
            account_id: "nope".into(),
            folder: "INBOX".into(),
            limit: None,
            offset: None,
        };
        assert_eq!(uc.risk(&input, &ctx).expect("risk"), Risk::Read);
        let out = uc.run(input, &ctx).await.expect("run");
        assert!(out.threads.is_empty());
        assert!(!out.has_more);
    }

    #[tokio::test]
    async fn test_get_unread_counts_usecase_is_read() {
        let (db, pending, batches, sync_locks) = build_states();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &sync_locks);
        let input = GetUnreadCountsInput {
            account_id: "acc1".into(),
        };
        assert_eq!(
            GetUnreadCountsUseCase.risk(&input, &ctx).expect("risk"),
            Risk::Read
        );
        let out = GetUnreadCountsUseCase.run(input, &ctx).await.expect("run");
        assert_eq!(out.unclassified, 0);
    }

    #[tokio::test]
    async fn test_get_threads_by_project_returns_empty_for_project_without_mails() {
        let (db, pending, batches, sync_locks) = build_states();
        db.with_conn(|conn| {
            crate::db::projects::insert_project_with_id(conn, "p1", "acc1", "P", None, None, None)?;
            Ok(())
        })
        .unwrap();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &sync_locks);
        let input = GetThreadsByProjectInput {
            project_id: "p1".into(),
            limit: None,
            offset: None,
        };
        assert_eq!(
            GetThreadsByProjectUseCase.risk(&input, &ctx).expect("risk"),
            Risk::Read
        );
        let out = GetThreadsByProjectUseCase
            .run(input, &ctx)
            .await
            .expect("run");
        assert!(out.threads.is_empty());
        assert!(!out.has_more);
    }

    #[tokio::test]
    async fn test_unarchive_moves_mail_back_to_inbox() {
        let (db, pending, batches, locks) = build_states();
        insert_mail_in_folder(&db, "m1", "Archive");
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks);

        UnarchiveMailUseCase
            .run(
                UnarchiveMailInput {
                    account_id: "acc1".into(),
                    mail_id: "m1".into(),
                },
                &ctx,
            )
            .await
            .expect("unarchive should succeed");

        let mail = db
            .with_conn(|conn| crate::db::mails::get_mail_by_id(conn, "m1"))
            .unwrap();
        assert_eq!(mail.folder, "INBOX");
    }

    // --- スレッド単位ページング（ADR 0006 決定5） ---

    /// limit / offset を省略しても既定の窓が適用される（上限なしにならない）
    #[test]
    fn test_resolve_page_window_defaults() {
        assert_eq!(
            resolve_page_window(None, None),
            (DEFAULT_THREAD_PAGE_SIZE, 0)
        );
        assert_eq!(resolve_page_window(Some(10), Some(20)), (10, 20));
    }

    /// CLI / MCP から巨大な limit が渡っても全件転送に退化しない
    #[test]
    fn test_resolve_page_window_clamps_to_max() {
        assert_eq!(
            resolve_page_window(Some(100_000), None),
            (MAX_THREAD_PAGE_SIZE, 0)
        );
    }

    /// 本 PR の設計判断の核心。スレッドは分断されない——5通のスレッドを
    /// limit=1 で取っても 5 通すべて揃って返る（メール単位LIMITなら欠ける）
    #[tokio::test]
    async fn test_get_threads_keeps_threads_whole_under_limit() {
        let (db, pending, batches, locks) = build_states();
        let root = make_mail("r1", "<r1@ex.com>", "長いスレッド", "2026-07-01T10:00:00");
        db.with_conn(|conn| crate::db::mails::insert_mail(conn, &root))
            .unwrap();
        for i in 0..4 {
            let mut reply = make_mail(
                &format!("c{i}"),
                &format!("<c{i}@ex.com>"),
                "Re: 長いスレッド",
                &format!("2026-07-01T1{}:00:00", i + 1),
            );
            reply.in_reply_to = Some("<r1@ex.com>".into());
            db.with_conn(|conn| crate::db::mails::insert_mail(conn, &reply))
                .unwrap();
        }
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks);

        let page = GetThreadsUseCase
            .run(
                GetThreadsInput {
                    account_id: "acc1".into(),
                    folder: "INBOX".into(),
                    limit: Some(1),
                    offset: None,
                },
                &ctx,
            )
            .await
            .expect("get_threads should succeed");

        assert_eq!(page.threads.len(), 1);
        assert_eq!(
            page.threads[0].mails.len(),
            5,
            "スレッドは分断されず全メールが揃う"
        );
        assert!(!page.has_more, "5通は1スレッドなので後続なし");
    }

    /// offset でページを進められ、ページ間で重複しない
    #[tokio::test]
    async fn test_get_threads_pages_without_overlap() {
        let (db, pending, batches, locks) = build_states();
        for i in 0..5 {
            let m = make_mail(
                &format!("m{i}"),
                &format!("<m{i}@ex.com>"),
                &format!("Subject {i}"),
                &format!("2026-07-01T0{i}:00:00"),
            );
            db.with_conn(|conn| crate::db::mails::insert_mail(conn, &m))
                .unwrap();
        }
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks);

        let run = |limit, offset| {
            GetThreadsUseCase.run(
                GetThreadsInput {
                    account_id: "acc1".into(),
                    folder: "INBOX".into(),
                    limit: Some(limit),
                    offset: Some(offset),
                },
                &ctx,
            )
        };

        let first = run(3, 0).await.unwrap();
        let second = run(3, 3).await.unwrap();
        assert_eq!(first.threads.len(), 3);
        assert!(first.has_more);
        assert_eq!(second.threads.len(), 2, "端数ページ");
        assert!(!second.has_more);

        let mut ids: Vec<String> = first
            .threads
            .iter()
            .chain(second.threads.iter())
            .map(|t| t.thread_id.clone())
            .collect();
        let total = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), total, "ページ間で重複しない");
        assert_eq!(total, 5);
    }

    /// 案件のスレッド一覧もスレッド単位で切れる
    #[tokio::test]
    async fn test_get_threads_by_project_pages() {
        let (db, pending, batches, locks) = build_states();
        db.with_conn(|conn| {
            crate::db::projects::insert_project_with_id(conn, "p1", "acc1", "P", None, None, None)
        })
        .unwrap();
        for i in 0..3 {
            let m = make_mail(
                &format!("m{i}"),
                &format!("<m{i}@ex.com>"),
                &format!("Subject {i}"),
                &format!("2026-07-01T0{i}:00:00"),
            );
            db.with_conn(|conn| crate::db::mails::insert_mail(conn, &m))
                .unwrap();
            db.with_conn(|conn| {
                crate::db::assignments::assign_mail(conn, &format!("m{i}"), "p1", "user", None)
            })
            .unwrap();
        }
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks);

        let page = GetThreadsByProjectUseCase
            .run(
                GetThreadsByProjectInput {
                    project_id: "p1".into(),
                    limit: Some(2),
                    offset: Some(0),
                },
                &ctx,
            )
            .await
            .expect("get_threads_by_project should succeed");

        assert_eq!(page.threads.len(), 2);
        assert!(page.has_more);
    }

    /// MCP の tools/list が返すスキーマに limit / offset が現れること。
    /// エージェントはこのスキーマだけを見てページングを組み立てる
    #[test]
    fn test_get_threads_input_schema_exposes_paging() {
        use crate::usecase::ErasedUseCase;
        let schema = ErasedUseCase::input_schema(&GetThreadsUseCase);
        let props = &schema["properties"];
        assert!(props.get("limit").is_some(), "limit がスキーマに出る");
        assert!(props.get("offset").is_some(), "offset がスキーマに出る");
        // 省略可能であること（required に含まれない）
        let required = schema["required"].as_array().cloned().unwrap_or_default();
        assert!(!required.iter().any(|v| v == "limit"));
        assert!(!required.iter().any(|v| v == "offset"));
    }
}
