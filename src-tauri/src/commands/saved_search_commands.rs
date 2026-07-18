//! 保存検索（スマートビュー）の Tauri commands。
//! 特権メール操作ではないため dispatch バスを介さず、projects と同じく
//! db 層へ直接委譲する薄いラッパ（本体ロジックは db::saved_searches が持つ）。

use tauri::State;

use crate::db::saved_searches;
use crate::models::saved_search::{CreateSavedSearchRequest, SavedSearch};
use crate::state::DbState;

#[tauri::command]
pub fn list_saved_searches(state: State<DbState>) -> Result<Vec<SavedSearch>, String> {
    state
        .with_conn(saved_searches::list_saved_searches)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_saved_search(
    state: State<DbState>,
    name: String,
    query: String,
    mode: String,
) -> Result<SavedSearch, String> {
    let req = CreateSavedSearchRequest { name, query, mode };
    state
        .with_conn(|conn| saved_searches::insert_saved_search(conn, &req))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn rename_saved_search(state: State<DbState>, id: i64, name: String) -> Result<(), String> {
    state
        .with_conn(|conn| saved_searches::rename_saved_search(conn, id, &name))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_saved_search(state: State<DbState>, id: i64) -> Result<(), String> {
    state
        .with_conn(|conn| saved_searches::delete_saved_search(conn, id))
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    // コマンド関数は State<DbState> を要求し単体では組み立てにくいため、
    // project_commands.rs の既存流儀に倣い、委譲先の db 層関数を直接叩いて
    // ラウンドトリップを検証する。
    use crate::test_helpers::setup_db;

    #[test]
    fn test_create_and_list_roundtrip() {
        let conn = setup_db();
        let created = crate::db::saved_searches::insert_saved_search(
            &conn,
            &crate::models::saved_search::CreateSavedSearchRequest {
                name: "照明".into(),
                query: "灯体".into(),
                mode: "semantic".into(),
            },
        )
        .unwrap();
        let listed = crate::db::saved_searches::list_saved_searches(&conn).unwrap();
        assert_eq!(listed[0].id, created.id);
    }
}
