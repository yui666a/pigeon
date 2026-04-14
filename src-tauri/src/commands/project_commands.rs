use tauri::State;

use crate::commands::account_commands::DbState;
use crate::db::projects;
use crate::models::project::{CreateProjectRequest, Project, UpdateProjectRequest};

#[tauri::command]
pub fn create_project(
    state: State<DbState>,
    account_id: String,
    name: String,
    description: Option<String>,
    color: Option<String>,
) -> Result<Project, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let req = CreateProjectRequest {
        account_id,
        name,
        description,
        color,
    };
    projects::insert_project(&conn, &req).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_projects(state: State<DbState>, account_id: String) -> Result<Vec<Project>, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    projects::list_projects(&conn, &account_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_project(
    state: State<DbState>,
    id: String,
    name: Option<String>,
    description: Option<String>,
    color: Option<String>,
) -> Result<Project, String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    let req = UpdateProjectRequest {
        name,
        description,
        color,
    };
    projects::update_project(&conn, &id, &req).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn archive_project(state: State<DbState>, id: String) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    projects::archive_project(&conn, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_project(state: State<DbState>, id: String) -> Result<(), String> {
    let conn = state.0.lock().map_err(|e| e.to_string())?;
    projects::delete_project(&conn, &id).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrations::run_migrations;
    use crate::models::project::UpdateProjectRequest;
    use rusqlite::Connection;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type, provider)
             VALUES ('acc1', 'Test', 'test@example.com', 'imap.example.com', 'smtp.example.com', 'plain', 'other')",
            [],
        ).unwrap();
        conn
    }

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
