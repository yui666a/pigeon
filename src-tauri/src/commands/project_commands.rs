use tauri::State;

use crate::classifier::service::{ClassifyBatches, PendingClassifications};
use crate::context::Ctx;
use crate::db::projects;
use crate::error::AppError;
use crate::models::project::Project;
use crate::state::{DbState, SecureStoreState, SyncLocks};
use crate::usecase::{dispatch, Registry};

#[tauri::command]
pub async fn create_project(
    registry: State<'_, Registry>,
    db: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    pending: State<'_, PendingClassifications>,
    batches: State<'_, ClassifyBatches>,
    sync_locks: State<'_, SyncLocks>,
    account_id: String,
    name: String,
    description: Option<String>,
    color: Option<String>,
    parent_id: Option<String>,
) -> Result<Project, AppError> {
    let ctx = Ctx::new(&db, &secure_store, &pending, &batches, &sync_locks);
    let out = dispatch(
        &registry,
        "create_project",
        serde_json::json!({
            "account_id": account_id, "name": name, "description": description,
            "color": color, "parent_id": parent_id,
        }),
        &ctx,
    )
    .await?;
    serde_json::from_value(out)
        .map_err(|e| AppError::Validation(format!("unexpected create_project output: {e}")))
}

#[tauri::command]
pub async fn get_projects(
    registry: State<'_, Registry>,
    db: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    pending: State<'_, PendingClassifications>,
    batches: State<'_, ClassifyBatches>,
    sync_locks: State<'_, SyncLocks>,
    account_id: String,
) -> Result<Vec<Project>, AppError> {
    let ctx = Ctx::new(&db, &secure_store, &pending, &batches, &sync_locks);
    let out = dispatch(
        &registry,
        "get_projects",
        serde_json::json!({ "account_id": account_id }),
        &ctx,
    )
    .await?;
    serde_json::from_value(out)
        .map_err(|e| AppError::Validation(format!("unexpected get_projects output: {e}")))
}

#[tauri::command]
pub async fn update_project(
    registry: State<'_, Registry>,
    db: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    pending: State<'_, PendingClassifications>,
    batches: State<'_, ClassifyBatches>,
    sync_locks: State<'_, SyncLocks>,
    id: String,
    name: Option<String>,
    description: Option<String>,
    color: Option<String>,
) -> Result<Project, AppError> {
    let ctx = Ctx::new(&db, &secure_store, &pending, &batches, &sync_locks);
    let out = dispatch(
        &registry,
        "update_project",
        serde_json::json!({
            "id": id, "name": name, "description": description, "color": color,
        }),
        &ctx,
    )
    .await?;
    serde_json::from_value(out)
        .map_err(|e| AppError::Validation(format!("unexpected update_project output: {e}")))
}

#[tauri::command]
pub async fn set_project_parent(
    registry: State<'_, Registry>,
    db: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    pending: State<'_, PendingClassifications>,
    batches: State<'_, ClassifyBatches>,
    sync_locks: State<'_, SyncLocks>,
    project_id: String,
    parent_id: Option<String>,
) -> Result<(), AppError> {
    let ctx = Ctx::new(&db, &secure_store, &pending, &batches, &sync_locks);
    let out = dispatch(
        &registry,
        "set_project_parent",
        serde_json::json!({ "project_id": project_id, "parent_id": parent_id }),
        &ctx,
    )
    .await?;
    serde_json::from_value(out)
        .map_err(|e| AppError::Validation(format!("unexpected set_project_parent output: {e}")))
}

#[tauri::command]
pub async fn archive_project(
    registry: State<'_, Registry>,
    db: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    pending: State<'_, PendingClassifications>,
    batches: State<'_, ClassifyBatches>,
    sync_locks: State<'_, SyncLocks>,
    id: String,
) -> Result<(), AppError> {
    let ctx = Ctx::new(&db, &secure_store, &pending, &batches, &sync_locks);
    let out = dispatch(
        &registry,
        "archive_project",
        serde_json::json!({ "project_id": id }),
        &ctx,
    )
    .await?;
    serde_json::from_value(out)
        .map_err(|e| AppError::Validation(format!("unexpected archive_project output: {e}")))
}

#[tauri::command]
pub async fn delete_project(
    registry: State<'_, Registry>,
    db: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    pending: State<'_, PendingClassifications>,
    batches: State<'_, ClassifyBatches>,
    sync_locks: State<'_, SyncLocks>,
    id: String,
) -> Result<(), AppError> {
    let ctx = Ctx::new(&db, &secure_store, &pending, &batches, &sync_locks);
    let out = dispatch(
        &registry,
        "delete_project",
        serde_json::json!({ "project_id": id }),
        &ctx,
    )
    .await?;
    serde_json::from_value(out)
        .map_err(|e| AppError::Validation(format!("unexpected delete_project output: {e}")))
}

/// Merge source project into target: reassign all mails, log corrections, delete source.
/// Returns the number of mails moved.
#[tauri::command]
pub async fn merge_projects(
    registry: State<'_, Registry>,
    db: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    pending: State<'_, PendingClassifications>,
    batches: State<'_, ClassifyBatches>,
    sync_locks: State<'_, SyncLocks>,
    source_id: String,
    target_id: String,
) -> Result<u32, AppError> {
    let ctx = Ctx::new(&db, &secure_store, &pending, &batches, &sync_locks);
    let out = dispatch(
        &registry,
        "merge_projects",
        serde_json::json!({ "source_id": source_id, "target_id": target_id }),
        &ctx,
    )
    .await?;
    serde_json::from_value(out)
        .map_err(|e| AppError::Validation(format!("unexpected merge_projects output: {e}")))
}

/// 案件の祖先パス（ルート→自ノード）に沿った加算的な有効コンテキストを返す。
/// 各エントリの定義元ノードが明示されるので、クラウド送信可否は呼び出し側が
/// 定義元ノードの `allow_cloud_context` に従って個別判定する（ルールの合成はしない）。
#[tauri::command]
pub fn get_effective_context(
    state: State<DbState>,
    project_id: String,
) -> Result<Vec<projects::EffectiveContextEntry>, AppError> {
    state.with_conn(|conn| projects::build_effective_context(conn, &project_id))
}

#[derive(serde::Serialize)]
pub struct DeleteImpact {
    pub projects: u32,
    pub mails: u32,
}

/// 削除確認ダイアログ用: サブツリーの案件数とメール件数を返す（Read 系・直呼び）。
#[tauri::command]
pub fn get_project_delete_impact(
    state: State<DbState>,
    project_id: String,
) -> Result<DeleteImpact, AppError> {
    state.with_conn(|conn| {
        let ids = projects::subtree_ids(conn, &project_id)?;
        if ids.is_empty() {
            return Err(AppError::ProjectNotFound(project_id.clone()));
        }
        Ok(DeleteImpact {
            projects: ids.len() as u32,
            mails: projects::count_subtree_mails(conn, &project_id)?,
        })
    })
}

#[cfg(test)]
mod tests {
    use crate::db::projects;
    use crate::models::project::{CreateProjectRequest, UpdateProjectRequest};
    use crate::test_helpers::setup_db;

    #[test]
    fn test_create_and_list_projects() {
        let conn = setup_db();
        let req = CreateProjectRequest {
            account_id: "acc1".into(),
            name: "Alpha".into(),
            description: Some("First".into()),
            color: Some("#ff0000".into()),
            parent_id: None,
        };
        let project = projects::insert_project(&conn, &req).unwrap();
        assert_eq!(project.name, "Alpha");
        assert_eq!(project.description, Some("First".into()));

        let list = projects::list_projects(&conn, "acc1").unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, project.id);
    }

    #[test]
    fn test_update_project() {
        let conn = setup_db();
        let req = CreateProjectRequest {
            account_id: "acc1".into(),
            name: "Old Name".into(),
            description: None,
            color: None,
            parent_id: None,
        };
        let project = projects::insert_project(&conn, &req).unwrap();

        let update = UpdateProjectRequest {
            name: Some("New Name".into()),
            description: Some("Added desc".into()),
            color: None,
        };
        let updated = projects::update_project(&conn, &project.id, &update).unwrap();
        assert_eq!(updated.name, "New Name");
        assert_eq!(updated.description, Some("Added desc".into()));
    }

    #[test]
    fn test_archive_project_removes_from_list() {
        let conn = setup_db();
        let req = CreateProjectRequest {
            account_id: "acc1".into(),
            name: "To Archive".into(),
            description: None,
            color: None,
            parent_id: None,
        };
        let project = projects::insert_project(&conn, &req).unwrap();
        projects::archive_project(&conn, &project.id).unwrap();

        // list_projects excludes archived
        let list = projects::list_projects(&conn, "acc1").unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn test_delete_project() {
        let conn = setup_db();
        let req = CreateProjectRequest {
            account_id: "acc1".into(),
            name: "To Delete".into(),
            description: None,
            color: None,
            parent_id: None,
        };
        let project = projects::insert_project(&conn, &req).unwrap();
        projects::delete_project(&conn, &project.id).unwrap();

        let list = projects::list_projects(&conn, "acc1").unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn test_list_projects_filters_by_account() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type, provider)
             VALUES ('acc2', 'Other', 'other@example.com', 'imap.example.com', 'smtp.example.com', 'plain', 'other')",
            [],
        ).unwrap();

        let req1 = CreateProjectRequest {
            account_id: "acc1".into(),
            name: "Acc1 Project".into(),
            description: None,
            color: None,
            parent_id: None,
        };
        let req2 = CreateProjectRequest {
            account_id: "acc2".into(),
            name: "Acc2 Project".into(),
            description: None,
            color: None,
            parent_id: None,
        };
        projects::insert_project(&conn, &req1).unwrap();
        projects::insert_project(&conn, &req2).unwrap();

        let list1 = projects::list_projects(&conn, "acc1").unwrap();
        let list2 = projects::list_projects(&conn, "acc2").unwrap();
        assert_eq!(list1.len(), 1);
        assert_eq!(list1[0].name, "Acc1 Project");
        assert_eq!(list2.len(), 1);
        assert_eq!(list2[0].name, "Acc2 Project");
    }
}
