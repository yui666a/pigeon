use tauri::State;

use crate::classifier::factory::build_classifier;
use crate::db::{cloud_rules, directories, project_contexts, project_files};
use crate::error::AppError;
use crate::models::directory::{CloudRule, ProjectContext, ProjectDirectory, ProjectFile};
use crate::project_context::{self, RescanOutcome};
use crate::state::{DbState, SecureStoreState};
use rusqlite::Connection;

/// パスを検証して紐付ける（コマンド本体から分離してテスト可能に）。
pub(crate) fn validate_and_link(
    conn: &mut Connection,
    project_id: &str,
    path: &str,
) -> Result<ProjectDirectory, AppError> {
    if !std::path::Path::new(path).is_absolute() {
        return Err(AppError::DirectoryScan(format!(
            "absolute path required: {}",
            path
        )));
    }
    directories::link_directory(conn, project_id, path)
}

pub(crate) fn apply_cloud_rule(
    conn: &Connection,
    directory_id: &str,
    scope: &str,
    relative_path: &str,
    allow: Option<bool>,
) -> Result<(), AppError> {
    match allow {
        Some(allow) => cloud_rules::set_rule(conn, directory_id, scope, relative_path, allow),
        None => cloud_rules::delete_rule(conn, directory_id, scope, relative_path),
    }
}

#[tauri::command]
pub fn link_project_directory(
    db: State<DbState>,
    project_id: String,
    path: String,
) -> Result<ProjectDirectory, AppError> {
    db.with_conn_mut(|conn| validate_and_link(conn, &project_id, &path))
}

#[tauri::command]
pub fn unlink_project_directory(db: State<DbState>, project_id: String) -> Result<(), AppError> {
    db.with_conn(|conn| directories::unlink_directory(conn, &project_id))
}

#[tauri::command]
pub fn get_project_directory(
    db: State<DbState>,
    project_id: String,
) -> Result<Option<ProjectDirectory>, AppError> {
    db.with_conn(|conn| directories::get_directory_by_project(conn, &project_id))
}

#[tauri::command]
pub async fn rescan_project_directory(
    db: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    project_id: String,
) -> Result<RescanOutcome, AppError> {
    let classifier = db.with_conn(|conn| build_classifier(conn, &secure_store.0))?;
    // プロバイダが Claude のときのみクラウド送信になる。cloud フラグは
    // 送信可否ポリシー適用のためのもので、build_classifier とは独立。
    let cloud = db.with_conn(|conn| {
        Ok(crate::db::settings::get_or_default(conn, "llm_provider", "ollama")? == "claude")
    })?;
    project_context::rescan_project(&db.0, classifier.as_ref(), &project_id, cloud).await
}

#[tauri::command]
pub fn list_project_files(
    db: State<DbState>,
    directory_id: String,
) -> Result<Vec<ProjectFile>, AppError> {
    db.with_conn(|conn| project_files::list_files(conn, &directory_id))
}

#[tauri::command]
pub fn set_cloud_rule(
    db: State<DbState>,
    directory_id: String,
    scope: String,
    relative_path: String,
    allow: Option<bool>,
) -> Result<(), AppError> {
    db.with_conn(|conn| apply_cloud_rule(conn, &directory_id, &scope, &relative_path, allow))
}

#[tauri::command]
pub fn get_cloud_rules(
    db: State<DbState>,
    directory_id: String,
) -> Result<Vec<CloudRule>, AppError> {
    db.with_conn(|conn| cloud_rules::list_rules(conn, &directory_id))
}

#[tauri::command]
pub fn set_allow_cloud_context(
    db: State<DbState>,
    project_id: String,
    allow: bool,
) -> Result<(), AppError> {
    db.with_conn(|conn| project_contexts::set_allow_cloud_context(conn, &project_id, allow))
}

#[tauri::command]
pub fn get_project_context(
    db: State<DbState>,
    project_id: String,
) -> Result<Option<ProjectContext>, AppError> {
    db.with_conn(|conn| project_contexts::get_context(conn, &project_id))
}

#[cfg(test)]
mod tests {
    use crate::db::{cloud_rules, directories};
    use crate::test_helpers::setup_db;

    #[test]
    fn test_set_cloud_rule_none_deletes() {
        let mut conn = setup_db();
        conn.execute(
            "INSERT INTO projects (id, account_id, name) VALUES ('p1', 'acc1', 'P')",
            [],
        )
        .unwrap();
        let dir = directories::link_directory(&mut conn, "p1", "/tmp/x").unwrap();

        super::apply_cloud_rule(&conn, &dir.id, "file", "a.txt", Some(true)).unwrap();
        assert_eq!(cloud_rules::list_rules(&conn, &dir.id).unwrap().len(), 1);

        super::apply_cloud_rule(&conn, &dir.id, "file", "a.txt", None).unwrap();
        assert!(cloud_rules::list_rules(&conn, &dir.id).unwrap().is_empty());
    }

    #[test]
    fn test_link_validates_path_is_absolute() {
        let mut conn = setup_db();
        conn.execute(
            "INSERT INTO projects (id, account_id, name) VALUES ('p1', 'acc1', 'P')",
            [],
        )
        .unwrap();
        let result = super::validate_and_link(&mut conn, "p1", "relative/path");
        assert!(result.is_err(), "相対パスは拒否する");
    }
}
