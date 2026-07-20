//! 案件構造の変更系 use case。階層を変更できる操作は全てここに集約し
//! dispatch バス（ADR 0004）経由にする——経路を割ると認可・監査が分裂するため。
//! Risk は設計書 §5 の表に従う: 作成/更新/付け替えは Reversible、
//! アーカイブ（復元なし）/削除（サブツリー+付随データ）/マージ（source 削除）は Sensitive。

use serde::Deserialize;

use crate::context::Ctx;
use crate::db::projects;
use crate::error::AppError;
use crate::models::project::{CreateProjectRequest, Project, UpdateProjectRequest};
use crate::usecase::{Registry, Risk, UseCase};

#[derive(Deserialize, schemars::JsonSchema)]
pub struct CreateProjectInput {
    pub account_id: String,
    pub name: String,
    pub description: Option<String>,
    pub color: Option<String>,
    pub parent_id: Option<String>,
}

pub struct CreateProjectUseCase;

#[async_trait::async_trait]
impl UseCase for CreateProjectUseCase {
    type Input = CreateProjectInput;
    type Output = Project;
    fn name(&self) -> &'static str {
        "create_project"
    }
    fn risk(&self, _input: &Self::Input, _ctx: &Ctx) -> Result<Risk, AppError> {
        Ok(Risk::Reversible)
    }
    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        let req = CreateProjectRequest {
            account_id: input.account_id,
            name: input.name,
            description: input.description,
            color: input.color,
            parent_id: input.parent_id,
        };
        ctx.with_conn(|conn| projects::insert_project(conn, &req))
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct UpdateProjectInput {
    pub id: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub color: Option<String>,
}

pub struct UpdateProjectUseCase;

#[async_trait::async_trait]
impl UseCase for UpdateProjectUseCase {
    type Input = UpdateProjectInput;
    type Output = Project;
    fn name(&self) -> &'static str {
        "update_project"
    }
    fn risk(&self, _input: &Self::Input, _ctx: &Ctx) -> Result<Risk, AppError> {
        Ok(Risk::Reversible)
    }
    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        let req = UpdateProjectRequest {
            name: input.name,
            description: input.description,
            color: input.color,
        };
        ctx.with_conn(|conn| projects::update_project(conn, &input.id, &req))
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct SetProjectParentInput {
    pub project_id: String,
    pub parent_id: Option<String>,
}

pub struct SetProjectParentUseCase;

#[async_trait::async_trait]
impl UseCase for SetProjectParentUseCase {
    type Input = SetProjectParentInput;
    type Output = ();
    fn name(&self) -> &'static str {
        "set_project_parent"
    }
    fn risk(&self, _input: &Self::Input, _ctx: &Ctx) -> Result<Risk, AppError> {
        Ok(Risk::Reversible)
    }
    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        ctx.with_conn(|conn| {
            projects::set_parent(conn, &input.project_id, input.parent_id.as_deref())
        })
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct ArchiveProjectInput {
    pub project_id: String,
}

pub struct ArchiveProjectUseCase;

#[async_trait::async_trait]
impl UseCase for ArchiveProjectUseCase {
    type Input = ArchiveProjectInput;
    type Output = ();
    fn name(&self) -> &'static str {
        "archive_project"
    }
    fn risk(&self, _input: &Self::Input, _ctx: &Ctx) -> Result<Risk, AppError> {
        // 復元 use case が無いため実質不可逆（設計書 §5）
        Ok(Risk::Sensitive)
    }
    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        ctx.with_conn(|conn| projects::archive_project(conn, &input.project_id))
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct DeleteProjectInput {
    pub project_id: String,
}

pub struct DeleteProjectUseCase;

#[async_trait::async_trait]
impl UseCase for DeleteProjectUseCase {
    type Input = DeleteProjectInput;
    type Output = ();
    fn name(&self) -> &'static str {
        "delete_project"
    }
    fn risk(&self, _input: &Self::Input, _ctx: &Ctx) -> Result<Risk, AppError> {
        Ok(Risk::Sensitive)
    }
    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        ctx.with_conn(|conn| projects::delete_project(conn, &input.project_id))
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct MergeProjectsInput {
    pub source_id: String,
    pub target_id: String,
}

pub struct MergeProjectsUseCase;

#[async_trait::async_trait]
impl UseCase for MergeProjectsUseCase {
    type Input = MergeProjectsInput;
    type Output = u32;
    fn name(&self) -> &'static str {
        "merge_projects"
    }
    fn risk(&self, _input: &Self::Input, _ctx: &Ctx) -> Result<Risk, AppError> {
        Ok(Risk::Sensitive)
    }
    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        ctx.with_conn(|conn| projects::merge_projects(conn, &input.source_id, &input.target_id))
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct GetProjectsInput {
    pub account_id: String,
}

/// アカウント配下の案件一覧（読み取り）。
pub struct GetProjectsUseCase;

#[async_trait::async_trait]
impl UseCase for GetProjectsUseCase {
    type Input = GetProjectsInput;
    type Output = Vec<Project>;
    fn name(&self) -> &'static str {
        "get_projects"
    }
    fn risk(&self, _input: &Self::Input, _ctx: &Ctx) -> Result<Risk, AppError> {
        Ok(Risk::Read)
    }
    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        ctx.with_conn(|conn| projects::list_projects(conn, &input.account_id))
    }
}

pub fn register_project_cases(registry: &mut Registry) {
    registry.register(GetProjectsUseCase);
    registry.register(CreateProjectUseCase);
    registry.register(UpdateProjectUseCase);
    registry.register(SetProjectParentUseCase);
    registry.register(ArchiveProjectUseCase);
    registry.register(DeleteProjectUseCase);
    registry.register(MergeProjectsUseCase);
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::classifier::service::{ClassifyBatches, PendingClassifications};
    use crate::db::projects;
    use crate::state::{DbState, SyncLocks};
    use crate::test_helpers::setup_db;
    use crate::usecase::{Risk, UseCase};

    fn build_states() -> (DbState, PendingClassifications, ClassifyBatches, SyncLocks) {
        (
            DbState(Mutex::new(setup_db())),
            PendingClassifications::new(),
            ClassifyBatches::new(),
            SyncLocks::new(),
        )
    }

    #[tokio::test]
    async fn test_create_project_usecase_with_parent() {
        let (db, pending, batches, locks) = build_states();
        db.with_conn(|conn| {
            projects::insert_project_with_id(conn, "root", "acc1", "ツアー", None, None, None)?;
            Ok(())
        })
        .unwrap();
        let ctx = crate::context::Ctx::new_for_test(&db, &pending, &batches, &locks);

        let out = CreateProjectUseCase
            .run(
                CreateProjectInput {
                    account_id: "acc1".into(),
                    name: "埼玉".into(),
                    description: None,
                    color: None,
                    parent_id: Some("root".into()),
                },
                &ctx,
            )
            .await
            .unwrap();
        assert_eq!(out.parent_id.as_deref(), Some("root"));
    }

    #[tokio::test]
    async fn test_set_project_parent_usecase() {
        let (db, pending, batches, locks) = build_states();
        db.with_conn(|conn| {
            projects::insert_project_with_id(conn, "a", "acc1", "A", None, None, None)?;
            projects::insert_project_with_id(conn, "b", "acc1", "B", None, None, None)?;
            Ok(())
        })
        .unwrap();
        let ctx = crate::context::Ctx::new_for_test(&db, &pending, &batches, &locks);

        SetProjectParentUseCase
            .run(
                SetProjectParentInput {
                    project_id: "b".into(),
                    parent_id: Some("a".into()),
                },
                &ctx,
            )
            .await
            .unwrap();
        let parent = db
            .with_conn(|conn| Ok(projects::get_project(conn, "b")?.parent_id))
            .unwrap();
        assert_eq!(parent.as_deref(), Some("a"));
    }

    #[tokio::test]
    async fn test_get_projects_usecase_lists_projects_of_account() {
        let (db, pending, batches, locks) = build_states();
        db.with_conn(|conn| {
            projects::insert_project_with_id(conn, "a", "acc1", "A", None, None, None)?;
            Ok(())
        })
        .unwrap();
        let ctx = crate::context::Ctx::new_for_test(&db, &pending, &batches, &locks);

        let input = GetProjectsInput {
            account_id: "acc1".into(),
        };
        assert_eq!(
            GetProjectsUseCase.risk(&input, &ctx).expect("risk"),
            Risk::Read
        );
        let out = GetProjectsUseCase.run(input, &ctx).await.expect("run");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, "a");
    }

    #[test]
    fn test_risk_matrix_matches_design() {
        // 設計書 §5: create/update/set_parent = Reversible、archive/delete/merge = Sensitive
        let (db, pending, batches, locks) = build_states();
        let ctx = crate::context::Ctx::new_for_test(&db, &pending, &batches, &locks);
        let dummy_create = CreateProjectInput {
            account_id: "acc1".into(),
            name: "n".into(),
            description: None,
            color: None,
            parent_id: None,
        };
        assert_eq!(
            CreateProjectUseCase.risk(&dummy_create, &ctx).unwrap(),
            Risk::Reversible
        );
        assert_eq!(
            DeleteProjectUseCase
                .risk(
                    &DeleteProjectInput {
                        project_id: "x".into()
                    },
                    &ctx
                )
                .unwrap(),
            Risk::Sensitive
        );
        assert_eq!(
            MergeProjectsUseCase
                .risk(
                    &MergeProjectsInput {
                        source_id: "a".into(),
                        target_id: "b".into()
                    },
                    &ctx
                )
                .unwrap(),
            Risk::Sensitive
        );
        assert_eq!(
            ArchiveProjectUseCase
                .risk(
                    &ArchiveProjectInput {
                        project_id: "x".into()
                    },
                    &ctx
                )
                .unwrap(),
            Risk::Sensitive
        );
    }
}
