//! 案件割り当て系の Reversible use case（Phase 4-3）。
//! 本体は classify_commands::move_mail_inner に委譲する
//! （保留中の分類提案の掃除も同じ挙動になる）。

use serde::Deserialize;

use crate::commands::bulk_commands::BulkResult;
use crate::commands::classify_commands::move_mail_inner;
use crate::context::Ctx;
use crate::error::AppError;
use crate::usecase::{Registry, Risk, UseCase};

#[derive(Deserialize)]
pub struct MoveMailInput {
    pub mail_id: String,
    pub project_id: String,
}

/// メールを案件へ割り当てる（IMAP 通信なし・ローカルのみ）。
pub struct MoveMailUseCase;

#[async_trait::async_trait]
impl UseCase for MoveMailUseCase {
    type Input = MoveMailInput;
    type Output = ();

    fn name(&self) -> &'static str {
        "move_mail"
    }

    fn risk(&self, _input: &Self::Input, _ctx: &Ctx) -> Result<Risk, AppError> {
        Ok(Risk::Reversible)
    }

    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        let pending = ctx.pending();
        ctx.with_conn(|conn| move_mail_inner(conn, pending, &input.mail_id, &input.project_id))
    }
}

#[derive(Deserialize)]
pub struct BulkMoveMailsInput {
    pub mail_ids: Vec<String>,
    pub project_id: String,
}

/// 複数メールを一括で案件へ割り当てる（部分失敗を積み上げて返す）。
pub struct BulkMoveMailsUseCase;

#[async_trait::async_trait]
impl UseCase for BulkMoveMailsUseCase {
    type Input = BulkMoveMailsInput;
    type Output = BulkResult;

    fn name(&self) -> &'static str {
        "bulk_move_mails"
    }

    fn risk(&self, _input: &Self::Input, _ctx: &Ctx) -> Result<Risk, AppError> {
        Ok(Risk::Reversible)
    }

    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        let pending = ctx.pending();
        ctx.with_conn(|conn| {
            let mut result = BulkResult::new();
            for mail_id in input.mail_ids {
                let outcome = move_mail_inner(conn, pending, &mail_id, &input.project_id);
                result.push(mail_id, outcome);
            }
            Ok(result)
        })
    }
}

pub fn register_assign_cases(registry: &mut Registry) {
    registry.register(MoveMailUseCase);
    registry.register(BulkMoveMailsUseCase);
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::classifier::service::{ClassifyBatches, PendingClassifications};
    use crate::db::{assignments, projects};
    use crate::models::project::CreateProjectRequest;
    use crate::state::{DbState, SyncLocks};
    use crate::test_helpers::{insert_test_mail, setup_db};

    fn build_states() -> (DbState, PendingClassifications, ClassifyBatches, SyncLocks) {
        (
            DbState(Mutex::new(setup_db())),
            PendingClassifications::new(),
            ClassifyBatches::new(),
            SyncLocks::new(),
        )
    }

    fn create_project(db: &DbState) -> String {
        db.with_conn(|conn| {
            let req = CreateProjectRequest {
                account_id: "acc1".into(),
                name: "Proj".into(),
                description: None,
                color: None,
                parent_id: None,
            };
            Ok(projects::insert_project(conn, &req)?.id)
        })
        .unwrap()
    }

    #[tokio::test]
    async fn test_move_mail_assigns_project() {
        let (db, pending, batches, locks) = build_states();
        db.with_conn(|conn| {
            insert_test_mail(conn, "m1", "Hello");
            Ok(())
        })
        .unwrap();
        let project_id = create_project(&db);
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks);

        MoveMailUseCase
            .run(
                MoveMailInput {
                    mail_id: "m1".into(),
                    project_id: project_id.clone(),
                },
                &ctx,
            )
            .await
            .expect("move should succeed");

        let assigned = db
            .with_conn(|conn| assignments::get_mails_by_project(conn, &project_id))
            .unwrap();
        assert_eq!(assigned.len(), 1);
    }

    #[tokio::test]
    async fn test_bulk_move_collects_partial_failures() {
        let (db, pending, batches, locks) = build_states();
        db.with_conn(|conn| {
            insert_test_mail(conn, "m1", "Hello");
            Ok(())
        })
        .unwrap();
        let project_id = create_project(&db);
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks);

        let result = BulkMoveMailsUseCase
            .run(
                BulkMoveMailsInput {
                    mail_ids: vec!["m1".into(), "ghost".into()],
                    project_id,
                },
                &ctx,
            )
            .await
            .expect("bulk move should not fail entirely");

        assert_eq!(result.succeeded, vec!["m1".to_string()]);
        assert_eq!(result.failed.len(), 1);
    }
}
