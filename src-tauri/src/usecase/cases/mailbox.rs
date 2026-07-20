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
use crate::models::mail::{Thread, UnreadCounts};
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

#[derive(Deserialize, schemars::JsonSchema)]
pub struct GetThreadsInput {
    pub account_id: String,
    pub folder: String,
}

/// フォルダ内メールをスレッド化して返す（読み取り）。
pub struct GetThreadsUseCase;

#[async_trait::async_trait]
impl UseCase for GetThreadsUseCase {
    type Input = GetThreadsInput;
    type Output = Vec<Thread>;

    fn name(&self) -> &'static str {
        "get_threads"
    }

    fn risk(&self, _input: &Self::Input, _ctx: &Ctx) -> Result<Risk, AppError> {
        Ok(Risk::Read)
    }

    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        // スレッド化は DB ロックの外で行う（接続を握ったままの CPU 処理を避ける）
        let all_mails = ctx.with_conn(|conn| {
            mails::get_mails_by_account(conn, &input.account_id, &input.folder)
        })?;
        Ok(mails::build_threads(&all_mails))
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct GetThreadsByProjectInput {
    pub project_id: String,
}

/// 案件に紐づくスレッド一覧（読み取り）。
pub struct GetThreadsByProjectUseCase;

#[async_trait::async_trait]
impl UseCase for GetThreadsByProjectUseCase {
    type Input = GetThreadsByProjectInput;
    type Output = Vec<Thread>;

    fn name(&self) -> &'static str {
        "get_threads_by_project"
    }

    fn risk(&self, _input: &Self::Input, _ctx: &Ctx) -> Result<Risk, AppError> {
        Ok(Risk::Read)
    }

    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        ctx.with_conn(|conn| mails::get_threads_by_project(conn, &input.project_id))
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
        };
        assert_eq!(uc.risk(&input, &ctx).expect("risk"), Risk::Read);
        let out = uc.run(input, &ctx).await.expect("run");
        assert!(out.is_empty());
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
        };
        assert_eq!(
            GetThreadsByProjectUseCase.risk(&input, &ctx).expect("risk"),
            Risk::Read
        );
        let out = GetThreadsByProjectUseCase
            .run(input, &ctx)
            .await
            .expect("run");
        assert!(out.is_empty());
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
}
