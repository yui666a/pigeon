use rusqlite::{params, Connection};
use uuid::Uuid;
use crate::error::AppError;
use crate::models::account::{Account, AuthType, CreateAccountRequest};

pub fn insert_account(conn: &Connection, req: &CreateAccountRequest) -> Result<Account, AppError> {
    let id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO accounts (id, name, email, imap_host, imap_port, smtp_host, smtp_port, auth_type)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![id, req.name, req.email, req.imap_host, req.imap_port, req.smtp_host, req.smtp_port, req.auth_type.as_str()],
    )?;
    get_account(conn, &id)
}

pub fn get_account(conn: &Connection, id: &str) -> Result<Account, AppError> {
    conn.query_row(
        "SELECT id, name, email, imap_host, imap_port, smtp_host, smtp_port, auth_type, created_at
         FROM accounts WHERE id = ?1",
        params![id],
        |row| {
            let auth_str: String = row.get(7)?;
            Ok(Account {
                id: row.get(0)?,
                name: row.get(1)?,
                email: row.get(2)?,
                imap_host: row.get(3)?,
                imap_port: row.get::<_, u32>(4)? as u16,
                smtp_host: row.get(5)?,
                smtp_port: row.get::<_, u32>(6)? as u16,
                auth_type: AuthType::try_from(auth_str.as_str()).unwrap_or(AuthType::Plain),
                created_at: row.get(8)?,
            })
        },
    ).map_err(|_| AppError::AccountNotFound(id.to_string()))
}

pub fn list_accounts(conn: &Connection) -> Result<Vec<Account>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, name, email, imap_host, imap_port, smtp_host, smtp_port, auth_type, created_at
         FROM accounts ORDER BY created_at",
    )?;
    let accounts = stmt
        .query_map([], |row| {
            let auth_str: String = row.get(7)?;
            Ok(Account {
                id: row.get(0)?,
                name: row.get(1)?,
                email: row.get(2)?,
                imap_host: row.get(3)?,
                imap_port: row.get::<_, u32>(4)? as u16,
                smtp_host: row.get(5)?,
                smtp_port: row.get::<_, u32>(6)? as u16,
                auth_type: AuthType::try_from(auth_str.as_str()).unwrap_or(AuthType::Plain),
                created_at: row.get(8)?,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(accounts)
}

pub fn delete_account(conn: &Connection, id: &str) -> Result<(), AppError> {
    let affected = conn.execute("DELETE FROM accounts WHERE id = ?1", params![id])?;
    if affected == 0 {
        return Err(AppError::AccountNotFound(id.to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrations::run_migrations;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        conn
    }

    fn sample_request() -> CreateAccountRequest {
        CreateAccountRequest {
            name: "Test Account".into(),
            email: "test@example.com".into(),
            imap_host: "imap.example.com".into(),
            imap_port: 993,
            smtp_host: "smtp.example.com".into(),
            smtp_port: 587,
            auth_type: AuthType::Plain,
            password: "secret".into(),
        }
    }

    #[test]
    fn test_insert_and_get_account() {
        let conn = setup_db();
        let account = insert_account(&conn, &sample_request()).unwrap();
        assert_eq!(account.name, "Test Account");
        assert_eq!(account.email, "test@example.com");
        let fetched = get_account(&conn, &account.id).unwrap();
        assert_eq!(fetched.id, account.id);
    }

    #[test]
    fn test_list_accounts() {
        let conn = setup_db();
        insert_account(&conn, &sample_request()).unwrap();
        let mut req2 = sample_request();
        req2.name = "Second Account".into();
        req2.email = "second@example.com".into();
        insert_account(&conn, &req2).unwrap();
        let accounts = list_accounts(&conn).unwrap();
        assert_eq!(accounts.len(), 2);
    }

    #[test]
    fn test_delete_account() {
        let conn = setup_db();
        let account = insert_account(&conn, &sample_request()).unwrap();
        delete_account(&conn, &account.id).unwrap();
        let result = get_account(&conn, &account.id);
        assert!(result.is_err());
    }

    #[test]
    fn test_get_nonexistent_account() {
        let conn = setup_db();
        let result = get_account(&conn, "nonexistent");
        assert!(result.is_err());
    }
}
