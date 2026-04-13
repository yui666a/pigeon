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
