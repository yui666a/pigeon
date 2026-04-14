use crate::db::assignments;
use crate::error::AppError;
use crate::models::classifier::ProjectSummary;
use crate::models::project::{CreateProjectRequest, Project, UpdateProjectRequest};
use rusqlite::{params, Connection};
use uuid::Uuid;

fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Project> {
    Ok(Project {
        id: row.get(0)?,
        account_id: row.get(1)?,
        name: row.get(2)?,
        description: row.get(3)?,
        color: row.get(4)?,
        is_archived: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

pub fn insert_project_with_id(
    conn: &Connection,
    id: &str,
    account_id: &str,
    name: &str,
    description: Option<&str>,
    color: Option<&str>,
) -> Result<Project, AppError> {
    conn.execute(
        "INSERT INTO projects (id, account_id, name, description, color)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, account_id, name, description, color],
    )?;
    get_project(conn, id)
}

pub fn insert_project(conn: &Connection, req: &CreateProjectRequest) -> Result<Project, AppError> {
    let id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO projects (id, account_id, name, description, color)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, req.account_id, req.name, req.description, req.color],
    )?;
    get_project(conn, &id)
}

pub fn get_project(conn: &Connection, id: &str) -> Result<Project, AppError> {
    conn.query_row(
        "SELECT id, account_id, name, description, color, is_archived, created_at, updated_at
         FROM projects WHERE id = ?1",
        params![id],
        map_row,
    )
    .map_err(|_| AppError::ProjectNotFound(id.to_string()))
}

pub fn list_projects(conn: &Connection, account_id: &str) -> Result<Vec<Project>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, account_id, name, description, color, is_archived, created_at, updated_at
         FROM projects
         WHERE account_id = ?1 AND is_archived = FALSE
         ORDER BY created_at",
    )?;
    let projects = stmt
        .query_map(params![account_id], map_row)?
        .filter_map(|r| r.ok())
        .collect();
    Ok(projects)
}

pub fn update_project(
    conn: &Connection,
    id: &str,
    req: &UpdateProjectRequest,
) -> Result<Project, AppError> {
    // Fetch existing project to apply partial updates
    let current = get_project(conn, id)?;

    let new_name = req.name.as_deref().unwrap_or(&current.name);
    let new_description = req.description.as_ref().or(current.description.as_ref());
    let new_color = req.color.as_ref().or(current.color.as_ref());

    conn.execute(
        "UPDATE projects
         SET name = ?1, description = ?2, color = ?3, updated_at = CURRENT_TIMESTAMP
         WHERE id = ?4",
        params![new_name, new_description, new_color, id],
    )?;
    get_project(conn, id)
}

pub fn archive_project(conn: &Connection, id: &str) -> Result<(), AppError> {
    let affected = conn.execute(
        "UPDATE projects SET is_archived = TRUE, updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
        params![id],
    )?;
    if affected == 0 {
        return Err(AppError::ProjectNotFound(id.to_string()));
    }
    Ok(())
}

/// Build ProjectSummary list for LLM classification context.
pub fn build_project_summaries(
    conn: &Connection,
    account_id: &str,
) -> Result<Vec<ProjectSummary>, AppError> {
    let projs = list_projects(conn, account_id)?;
    let mut summaries = Vec::with_capacity(projs.len());
    for p in projs {
        let recent_subjects =
            assignments::get_recent_subjects(conn, &p.id, 5).unwrap_or_default();
        summaries.push(ProjectSummary {
            id: p.id,
            name: p.name,
            description: p.description,
            recent_subjects,
        });
    }
    Ok(summaries)
}

pub fn delete_project(conn: &Connection, id: &str) -> Result<(), AppError> {
    let affected = conn.execute("DELETE FROM projects WHERE id = ?1", params![id])?;
    if affected == 0 {
        return Err(AppError::ProjectNotFound(id.to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::accounts::insert_account_with_id;
    use crate::db::migrations::run_migrations;
    use crate::models::account::{AccountProvider, AuthType, CreateAccountRequest};

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        run_migrations(&conn).unwrap();
        conn
    }

    fn create_test_account(conn: &Connection, id: &str) {
        let req = CreateAccountRequest {
            name: "Test Account".into(),
            email: format!("{}@example.com", id),
            imap_host: "imap.example.com".into(),
            imap_port: 993,
            smtp_host: "smtp.example.com".into(),
            smtp_port: 587,
            auth_type: AuthType::Plain,
            provider: AccountProvider::Other,
            password: None,
        };
        insert_account_with_id(conn, id, &req).unwrap();
    }

    fn sample_create_req(account_id: &str) -> CreateProjectRequest {
        CreateProjectRequest {
            account_id: account_id.to_string(),
            name: "Test Project".into(),
            description: Some("A test project".into()),
            color: Some("#FF5733".into()),
        }
    }

    #[test]
    fn test_insert_and_get_project() {
        let conn = setup_db();
        create_test_account(&conn, "acc1");

        let req = sample_create_req("acc1");
        let project = insert_project(&conn, &req).unwrap();

        assert!(!project.id.is_empty());
        assert_eq!(project.account_id, "acc1");
        assert_eq!(project.name, "Test Project");
        assert_eq!(project.description, Some("A test project".into()));
        assert_eq!(project.color, Some("#FF5733".into()));
        assert!(!project.is_archived);
        assert!(!project.created_at.is_empty());
        assert!(!project.updated_at.is_empty());

        let fetched = get_project(&conn, &project.id).unwrap();
        assert_eq!(fetched.id, project.id);
        assert_eq!(fetched.name, project.name);
    }

    #[test]
    fn test_list_projects_excludes_archived() {
        let conn = setup_db();
        create_test_account(&conn, "acc1");

        let req = sample_create_req("acc1");
        let p1 = insert_project(&conn, &req).unwrap();
        let p2 = insert_project(
            &conn,
            &CreateProjectRequest {
                account_id: "acc1".into(),
                name: "Project 2".into(),
                description: None,
                color: None,
            },
        )
        .unwrap();

        // Archive p2
        archive_project(&conn, &p2.id).unwrap();

        let projects = list_projects(&conn, "acc1").unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].id, p1.id);
    }

    #[test]
    fn test_update_project() {
        let conn = setup_db();
        create_test_account(&conn, "acc1");

        let project = insert_project(&conn, &sample_create_req("acc1")).unwrap();

        let update_req = UpdateProjectRequest {
            name: Some("Updated Name".into()),
            description: None,
            color: Some("#00FF00".into()),
        };
        let updated = update_project(&conn, &project.id, &update_req).unwrap();

        assert_eq!(updated.name, "Updated Name");
        assert_eq!(updated.description, Some("A test project".into())); // unchanged
        assert_eq!(updated.color, Some("#00FF00".into()));
    }

    #[test]
    fn test_delete_project() {
        let conn = setup_db();
        create_test_account(&conn, "acc1");

        let project = insert_project(&conn, &sample_create_req("acc1")).unwrap();
        delete_project(&conn, &project.id).unwrap();

        let result = get_project(&conn, &project.id);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AppError::ProjectNotFound(_)));
    }

    #[test]
    fn test_get_nonexistent_project() {
        let conn = setup_db();

        let result = get_project(&conn, "nonexistent-id");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AppError::ProjectNotFound(_)));
    }
}
