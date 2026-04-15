use tauri::State;

use crate::db::projects;
use crate::error::AppError;
use crate::models::project::{CreateProjectRequest, Project, UpdateProjectRequest};
use crate::state::DbState;

#[tauri::command]
pub fn create_project(
    state: State<DbState>,
    account_id: String,
    name: String,
    description: Option<String>,
    color: Option<String>,
) -> Result<Project, AppError> {
    let conn = state.0.lock().map_err(AppError::lock_err)?;
    let req = CreateProjectRequest {
        account_id,
        name,
        description,
        color,
    };
    Ok(projects::insert_project(&conn, &req)?)
}

#[tauri::command]
pub fn get_projects(state: State<DbState>, account_id: String) -> Result<Vec<Project>, AppError> {
    let conn = state.0.lock().map_err(AppError::lock_err)?;
    Ok(projects::list_projects(&conn, &account_id)?)
}

#[tauri::command]
pub fn update_project(
    state: State<DbState>,
    id: String,
    name: Option<String>,
    description: Option<String>,
    color: Option<String>,
) -> Result<Project, AppError> {
    let conn = state.0.lock().map_err(AppError::lock_err)?;
    let req = UpdateProjectRequest {
        name,
        description,
        color,
    };
    Ok(projects::update_project(&conn, &id, &req)?)
}

#[tauri::command]
pub fn archive_project(state: State<DbState>, id: String) -> Result<(), AppError> {
    let conn = state.0.lock().map_err(AppError::lock_err)?;
    Ok(projects::archive_project(&conn, &id)?)
}

#[tauri::command]
pub fn delete_project(state: State<DbState>, id: String) -> Result<(), AppError> {
    let conn = state.0.lock().map_err(AppError::lock_err)?;
    Ok(projects::delete_project(&conn, &id)?)
}

/// Merge source project into target: reassign all mails, log corrections, delete source.
/// Returns the number of mails moved.
#[tauri::command]
pub fn merge_projects(
    state: State<DbState>,
    source_id: String,
    target_id: String,
) -> Result<u32, AppError> {
    let mut conn = state.0.lock().map_err(AppError::lock_err)?;
    Ok(projects::merge_projects(&mut conn, &source_id, &target_id)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::project::UpdateProjectRequest;
    use crate::test_helpers::setup_db;

    #[test]
    fn test_create_and_list_projects() {
        let conn = setup_db();
        let req = CreateProjectRequest {
            account_id: "acc1".into(),
            name: "Alpha".into(),
            description: Some("First".into()),
            color: Some("#ff0000".into()),
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
        };
        let req2 = CreateProjectRequest {
            account_id: "acc2".into(),
            name: "Acc2 Project".into(),
            description: None,
            color: None,
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
