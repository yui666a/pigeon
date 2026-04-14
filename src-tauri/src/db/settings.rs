use rusqlite::{params, Connection};

/// Query the settings table for `key`, returning `default` if the row doesn't exist.
pub fn get_or_default(conn: &Connection, key: &str, default: &str) -> String {
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        params![key],
        |row| row.get::<_, String>(0),
    )
    .unwrap_or_else(|_| default.to_string())
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
}
