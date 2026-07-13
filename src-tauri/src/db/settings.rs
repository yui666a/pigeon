use crate::error::AppError;
use rusqlite::{params, Connection, OptionalExtension};

/// Query the settings table for `key`, returning `default` if the row doesn't exist.
/// 「行なし」のみ default にフォールバックし、DB ロック等の実エラーは伝播する（B-10）。
pub fn get_or_default(conn: &Connection, key: &str, default: &str) -> Result<String, AppError> {
    let value = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    Ok(value.unwrap_or_else(|| default.to_string()))
}

/// `key` の値を u32 として読む。未設定・数値でない場合は `default`。
pub fn get_u32_or(conn: &Connection, key: &str, default: u32) -> Result<u32, AppError> {
    Ok(get_or_default(conn, key, &default.to_string())?
        .parse()
        .unwrap_or(default))
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
        assert_eq!(get_or_default(&conn, "missing", "fallback").unwrap(), "fallback");
    }

    #[test]
    fn test_returns_stored_value() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('my_key', 'my_value')",
            [],
        )
        .unwrap();
        assert_eq!(get_or_default(&conn, "my_key", "default").unwrap(), "my_value");
    }

    #[test]
    fn test_get_u32_or_returns_default_when_missing() {
        let conn = setup_db();
        assert_eq!(get_u32_or(&conn, "initial_sync_limit", 5000).unwrap(), 5000);
    }

    #[test]
    fn test_get_u32_or_parses_stored_value() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('initial_sync_limit', '300')",
            [],
        )
        .unwrap();
        assert_eq!(get_u32_or(&conn, "initial_sync_limit", 5000).unwrap(), 300);
    }

    #[test]
    fn test_get_u32_or_falls_back_on_invalid_value() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('initial_sync_limit', 'abc')",
            [],
        )
        .unwrap();
        assert_eq!(get_u32_or(&conn, "initial_sync_limit", 5000).unwrap(), 5000);
    }

    #[test]
    fn test_get_or_default_propagates_real_errors() {
        // B-10: 「行なし」以外の障害（テーブル破壊等）まで default に丸めない
        let conn = setup_db();
        conn.execute_batch("DROP TABLE settings").unwrap();
        assert!(get_or_default(&conn, "llm_provider", "ollama").is_err());
        assert!(get_u32_or(&conn, "initial_sync_limit", 5000).is_err());
    }

    #[test]
    fn test_set_inserts_new_key() {
        let conn = setup_db();
        set(&conn, "llm_provider", "claude").unwrap();
        assert_eq!(get_or_default(&conn, "llm_provider", "ollama").unwrap(), "claude");
    }

    #[test]
    fn test_set_overwrites_existing_key() {
        let conn = setup_db();
        set(&conn, "llm_provider", "ollama").unwrap();
        set(&conn, "llm_provider", "claude").unwrap();
        assert_eq!(get_or_default(&conn, "llm_provider", "ollama").unwrap(), "claude");
    }
}
