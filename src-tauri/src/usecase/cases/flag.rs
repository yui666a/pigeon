//! フラグ・既読系の Reversible use case（Phase 4-3 Sensitive 抽出）。
//! 本体は commands 側の service 関数（set_flagged_service 等）に委譲する。

use serde::Deserialize;

use crate::commands::flag_commands::{mark_unread_service, set_flagged_service};
use crate::commands::mail_commands::mark_read_service;
use crate::context::Ctx;
use crate::error::AppError;
use crate::usecase::{Registry, Risk, UseCase};

#[derive(Deserialize, schemars::JsonSchema)]
pub struct SetFlaggedInput {
    pub account_id: String,
    pub mail_id: String,
    pub flagged: bool,
}

/// スター/フラグ（\Flagged）の付与・除去。
pub struct SetFlaggedUseCase;

#[async_trait::async_trait]
impl UseCase for SetFlaggedUseCase {
    type Input = SetFlaggedInput;
    type Output = ();

    fn name(&self) -> &'static str {
        "set_flagged"
    }

    fn risk(&self, _input: &Self::Input, _ctx: &Ctx) -> Result<Risk, AppError> {
        Ok(Risk::Reversible)
    }

    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        set_flagged_service(
            ctx.db(),
            ctx.secure_store()?,
            &input.account_id,
            &input.mail_id,
            input.flagged,
        )
        .await
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct MarkReadInput {
    pub account_id: String,
    pub mail_id: String,
}

/// 既読化。
pub struct MarkReadUseCase;

#[async_trait::async_trait]
impl UseCase for MarkReadUseCase {
    type Input = MarkReadInput;
    type Output = ();

    fn name(&self) -> &'static str {
        "mark_read"
    }

    fn risk(&self, _input: &Self::Input, _ctx: &Ctx) -> Result<Risk, AppError> {
        Ok(Risk::Reversible)
    }

    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        mark_read_service(
            ctx.db(),
            ctx.secure_store()?,
            &input.account_id,
            &input.mail_id,
        )
        .await
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct MarkUnreadInput {
    pub account_id: String,
    pub mail_id: String,
}

/// 未読に戻す。
pub struct MarkUnreadUseCase;

#[async_trait::async_trait]
impl UseCase for MarkUnreadUseCase {
    type Input = MarkUnreadInput;
    type Output = ();

    fn name(&self) -> &'static str {
        "mark_unread"
    }

    fn risk(&self, _input: &Self::Input, _ctx: &Ctx) -> Result<Risk, AppError> {
        Ok(Risk::Reversible)
    }

    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        mark_unread_service(
            ctx.db(),
            ctx.secure_store()?,
            &input.account_id,
            &input.mail_id,
        )
        .await
    }
}

pub fn register_flag_cases(registry: &mut Registry) {
    registry.register(SetFlaggedUseCase);
    registry.register(MarkReadUseCase);
    registry.register(MarkUnreadUseCase);
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::classifier::service::{ClassifyBatches, PendingClassifications};
    use crate::db::mails;
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

    /// Sent（ローカルのみ反映のフォルダ）のメールを入れる。IMAP 接続に進まないため
    /// テストで run を最後まで実行できる（local-only 分岐は mail_policy が根拠）。
    fn insert_sent_mail(db: &DbState, id: &str) {
        let mut mail = make_mail(
            id,
            &format!("<{id}@ex.com>"),
            "Subject",
            "2026-07-15T10:00:00",
        );
        mail.folder = "Sent".into();
        db.with_conn(|conn| mails::insert_mail(conn, &mail))
            .unwrap();
    }

    #[test]
    fn test_flag_cases_declare_reversible() {
        let (db, pending, batches, locks) = build_states();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks);
        let flag_input = SetFlaggedInput {
            account_id: "acc1".into(),
            mail_id: "m1".into(),
            flagged: true,
        };
        let read_input = MarkReadInput {
            account_id: "acc1".into(),
            mail_id: "m1".into(),
        };
        let unread_input = MarkUnreadInput {
            account_id: "acc1".into(),
            mail_id: "m1".into(),
        };
        assert_eq!(
            SetFlaggedUseCase.risk(&flag_input, &ctx).unwrap(),
            Risk::Reversible
        );
        assert_eq!(
            MarkReadUseCase.risk(&read_input, &ctx).unwrap(),
            Risk::Reversible
        );
        assert_eq!(
            MarkUnreadUseCase.risk(&unread_input, &ctx).unwrap(),
            Risk::Reversible
        );
    }

    #[tokio::test]
    async fn test_set_flagged_updates_db_for_local_only_folder() {
        let (db, pending, batches, locks) = build_states();
        insert_sent_mail(&db, "m1");
        let store = test_secure_store();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks).with_secure_store(&store);

        SetFlaggedUseCase
            .run(
                SetFlaggedInput {
                    account_id: "acc1".into(),
                    mail_id: "m1".into(),
                    flagged: true,
                },
                &ctx,
            )
            .await
            .expect("set_flagged on Sent should complete locally");

        let mail = db
            .with_conn(|conn| mails::get_mail_by_id(conn, "m1"))
            .unwrap();
        assert!(mail.is_flagged);
    }

    #[tokio::test]
    async fn test_mark_read_and_unread_roundtrip_for_local_only_folder() {
        let (db, pending, batches, locks) = build_states();
        insert_sent_mail(&db, "m1");
        let store = test_secure_store();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks).with_secure_store(&store);

        MarkReadUseCase
            .run(
                MarkReadInput {
                    account_id: "acc1".into(),
                    mail_id: "m1".into(),
                },
                &ctx,
            )
            .await
            .expect("mark_read on Sent should complete locally");
        let mail = db
            .with_conn(|conn| mails::get_mail_by_id(conn, "m1"))
            .unwrap();
        assert!(mail.is_read);

        MarkUnreadUseCase
            .run(
                MarkUnreadInput {
                    account_id: "acc1".into(),
                    mail_id: "m1".into(),
                },
                &ctx,
            )
            .await
            .expect("mark_unread on Sent should complete locally");
        let mail = db
            .with_conn(|conn| mails::get_mail_by_id(conn, "m1"))
            .unwrap();
        assert!(!mail.is_read);
    }
}
