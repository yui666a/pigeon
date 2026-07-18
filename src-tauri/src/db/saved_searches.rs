//! saved_searches（スマートビュー＝保存検索）の CRUD。
//! 「検索の保存」であり新しい概念を増やさない（設計書 Phase 3）。
//! 特権メール操作ではないため dispatch バスではなく projects と同じ直 CRUD。

use crate::error::AppError;
use crate::models::saved_search::{CreateSavedSearchRequest, SavedSearch};
use rusqlite::{params, Connection};

fn row_to_saved_search(row: &rusqlite::Row<'_>) -> rusqlite::Result<SavedSearch> {
    Ok(SavedSearch {
        id: row.get(0)?,
        name: row.get(1)?,
        query: row.get(2)?,
        mode: row.get(3)?,
        sort_order: row.get(4)?,
        created_at: row.get(5)?,
    })
}

const COLS: &str = "id, name, query, mode, sort_order, created_at";

pub fn list_saved_searches(conn: &Connection) -> Result<Vec<SavedSearch>, AppError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM saved_searches ORDER BY sort_order, id"
    ))?;
    let rows = stmt
        .query_map([], row_to_saved_search)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn insert_saved_search(
    conn: &Connection,
    req: &CreateSavedSearchRequest,
) -> Result<SavedSearch, AppError> {
    conn.execute(
        "INSERT INTO saved_searches (name, query, mode) VALUES (?1, ?2, ?3)",
        params![req.name, req.query, req.mode],
    )?;
    let id = conn.last_insert_rowid();
    let s = conn.query_row(
        &format!("SELECT {COLS} FROM saved_searches WHERE id = ?1"),
        [id],
        row_to_saved_search,
    )?;
    Ok(s)
}

pub fn rename_saved_search(conn: &Connection, id: i64, name: &str) -> Result<(), AppError> {
    // projects.rs の archive_project / delete_project と同じく、対象 0 行を
    // エラーで返す。あちらは専用の ProjectNotFound を使う流儀なので、saved_searches
    // でも同じく専用の SavedSearchNotFound を返す（Validation は入力不正の意味に
    // なり未存在の表現として不適切なため）。
    let affected = conn.execute(
        "UPDATE saved_searches SET name = ?1 WHERE id = ?2",
        params![name, id],
    )?;
    if affected == 0 {
        return Err(AppError::SavedSearchNotFound(format!("{id}")));
    }
    Ok(())
}

pub fn delete_saved_search(conn: &Connection, id: i64) -> Result<(), AppError> {
    let affected = conn.execute("DELETE FROM saved_searches WHERE id = ?1", params![id])?;
    if affected == 0 {
        return Err(AppError::SavedSearchNotFound(format!("{id}")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::setup_db;

    fn req(name: &str, mode: &str) -> CreateSavedSearchRequest {
        CreateSavedSearchRequest {
            name: name.into(),
            query: "照明".into(),
            mode: mode.into(),
        }
    }

    #[test]
    fn test_insert_and_list() {
        let conn = setup_db();
        let s = insert_saved_search(&conn, &req("照明の件", "semantic")).unwrap();
        assert_eq!(s.name, "照明の件");
        assert_eq!(s.mode, "semantic");
        let all = list_saved_searches(&conn).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].query, "照明");
    }

    #[test]
    fn test_invalid_mode_rejected_by_check() {
        let conn = setup_db();
        assert!(insert_saved_search(&conn, &req("x", "hybrid")).is_err());
    }

    #[test]
    fn test_rename() {
        let conn = setup_db();
        let s = insert_saved_search(&conn, &req("旧名", "fulltext")).unwrap();
        rename_saved_search(&conn, s.id, "新名").unwrap();
        assert_eq!(list_saved_searches(&conn).unwrap()[0].name, "新名");
    }

    #[test]
    fn test_rename_missing_is_error() {
        let conn = setup_db();
        let err = rename_saved_search(&conn, 9999, "x").unwrap_err();
        assert!(matches!(err, AppError::SavedSearchNotFound(_)));
    }

    #[test]
    fn test_delete() {
        let conn = setup_db();
        let s = insert_saved_search(&conn, &req("消す", "fulltext")).unwrap();
        delete_saved_search(&conn, s.id).unwrap();
        assert!(list_saved_searches(&conn).unwrap().is_empty());
        let err = delete_saved_search(&conn, s.id).unwrap_err();
        assert!(
            matches!(err, AppError::SavedSearchNotFound(_)),
            "二重削除は SavedSearchNotFound"
        );
    }

    #[test]
    fn test_list_orders_by_sort_order_then_id() {
        let conn = setup_db();
        let a = insert_saved_search(&conn, &req("a", "fulltext")).unwrap();
        let _b = insert_saved_search(&conn, &req("b", "fulltext")).unwrap();
        conn.execute(
            "UPDATE saved_searches SET sort_order = 10 WHERE id = ?1",
            [a.id],
        )
        .unwrap();
        let all = list_saved_searches(&conn).unwrap();
        assert_eq!(all[0].name, "b");
        assert_eq!(all[1].name, "a");
    }
}
