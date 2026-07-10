use rusqlite::{params, Connection};
use crate::error::AppError;

/// Query the settings table for `key`, returning `default` if the row doesn't exist.
pub fn get_or_default(conn: &Connection, key: &str, default: &str) -> String {
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        params![key],
        |row| row.get::<_, String>(0),
    )
    .unwrap_or_else(|_| default.to_string())
}

/// `key` の値を u32 として読む。未設定・数値でない場合は `default`。
pub fn get_u32_or(conn: &Connection, key: &str, default: u32) -> u32 {
    get_or_default(conn, key, &default.to_string())
        .parse()
        .unwrap_or(default)
}

/// `key` に `value` を UPSERT する。
pub fn set(conn: &Connection, key: &str, value: &str) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
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

    #[test]
    fn test_returns_default_when_missing() {
        let conn = setup_db();
        assert_eq!(get_or_default(&conn, "missing", "fallback"), "fallback");
    }

    #[test]
    fn test_returns_stored_value() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('my_key', 'my_value')",
            [],
        )
        .unwrap();
        assert_eq!(get_or_default(&conn, "my_key", "default"), "my_value");
    }

    #[test]
    fn test_get_u32_or_returns_default_when_missing() {
        let conn = setup_db();
        assert_eq!(get_u32_or(&conn, "initial_sync_limit", 5000), 5000);
    }

    #[test]
    fn test_get_u32_or_parses_stored_value() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('initial_sync_limit', '300')",
            [],
        )
        .unwrap();
        assert_eq!(get_u32_or(&conn, "initial_sync_limit", 5000), 300);
    }

    #[test]
    fn test_get_u32_or_falls_back_on_invalid_value() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('initial_sync_limit', 'abc')",
            [],
        )
        .unwrap();
        assert_eq!(get_u32_or(&conn, "initial_sync_limit", 5000), 5000);
    }

    #[test]
    fn test_set_inserts_new_key() {
        let conn = setup_db();
        set(&conn, "llm_provider", "claude").unwrap();
        assert_eq!(get_or_default(&conn, "llm_provider", "ollama"), "claude");
    }

    #[test]
    fn test_set_overwrites_existing_key() {
        let conn = setup_db();
        set(&conn, "llm_provider", "ollama").unwrap();
        set(&conn, "llm_provider", "claude").unwrap();
        assert_eq!(get_or_default(&conn, "llm_provider", "ollama"), "claude");
    }
}
