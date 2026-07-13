use crate::error::AppError;
use crate::models::directory::ProjectContext;
use rusqlite::{params, Connection, OptionalExtension};

/// project_contexts テーブルの1行を ProjectContext へ変換する共通マッパー。
/// カラム順は `SELECT project_id, cached_context, context_hash, inventory_hash,
/// allow_cloud_context, generated_at` に一致させること。
fn row_to_project_context(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProjectContext> {
    Ok(ProjectContext {
        project_id: row.get(0)?,
        cached_context: row.get(1)?,
        context_hash: row.get(2)?,
        inventory_hash: row.get(3)?,
        allow_cloud_context: row.get(4)?,
        generated_at: row.get(5)?,
    })
}

pub fn get_context(conn: &Connection, project_id: &str) -> Result<Option<ProjectContext>, AppError> {
    conn.query_row(
        "SELECT project_id, cached_context, context_hash, inventory_hash,
                allow_cloud_context, generated_at
         FROM project_contexts WHERE project_id = ?1",
        params![project_id],
        row_to_project_context,
    )
    .optional()
    .map_err(AppError::Database)
}

pub fn upsert_generated(
    conn: &Connection,
    project_id: &str,
    cached_context: &str,
    context_hash: &str,
    inventory_hash: &str,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO project_contexts
            (project_id, cached_context, context_hash, inventory_hash, generated_at)
         VALUES (?1, ?2, ?3, ?4, CURRENT_TIMESTAMP)
         ON CONFLICT(project_id) DO UPDATE SET
            cached_context = ?2, context_hash = ?3, inventory_hash = ?4,
            generated_at = CURRENT_TIMESTAMP",
        params![project_id, cached_context, context_hash, inventory_hash],
    )?;
    Ok(())
}

/// 自己修復用: PIGEON-CONTEXT.md の外部編集を検知したときにキャッシュだけ更新する。
pub fn update_cache_only(
    conn: &Connection,
    project_id: &str,
    cached_context: &str,
    context_hash: &str,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO project_contexts (project_id, cached_context, context_hash)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(project_id) DO UPDATE SET cached_context = ?2, context_hash = ?3",
        params![project_id, cached_context, context_hash],
    )?;
    Ok(())
}

pub fn set_allow_cloud_context(
    conn: &Connection,
    project_id: &str,
    allow: bool,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO project_contexts (project_id, allow_cloud_context)
         VALUES (?1, ?2)
         ON CONFLICT(project_id) DO UPDATE SET allow_cloud_context = ?2",
        params![project_id, allow],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::setup_db;

    fn create_project(conn: &Connection) {
        conn.execute(
            "INSERT INTO projects (id, account_id, name) VALUES ('p1', 'acc1', 'Proj')",
            [],
        )
        .unwrap();
    }

    #[test]
    fn test_get_context_none_initially() {
        let conn = setup_db();
        create_project(&conn);
        assert!(get_context(&conn, "p1").unwrap().is_none());
    }

    #[test]
    fn test_upsert_generated_and_get() {
        let conn = setup_db();
        create_project(&conn);

        upsert_generated(&conn, "p1", "コンテキスト", "chash1", "ihash1").unwrap();
        let ctx = get_context(&conn, "p1").unwrap().unwrap();
        assert_eq!(ctx.cached_context.as_deref(), Some("コンテキスト"));
        assert_eq!(ctx.context_hash.as_deref(), Some("chash1"));
        assert_eq!(ctx.inventory_hash.as_deref(), Some("ihash1"));
        assert!(!ctx.allow_cloud_context, "デフォルトは送信不許可");
        assert!(ctx.generated_at.is_some());

        // 2回目は上書き
        upsert_generated(&conn, "p1", "更新後", "chash2", "ihash2").unwrap();
        let ctx = get_context(&conn, "p1").unwrap().unwrap();
        assert_eq!(ctx.cached_context.as_deref(), Some("更新後"));
    }

    #[test]
    fn test_set_allow_cloud_context_survives_upsert() {
        let conn = setup_db();
        create_project(&conn);
        upsert_generated(&conn, "p1", "c", "h", "i").unwrap();
        set_allow_cloud_context(&conn, "p1", true).unwrap();
        // 再生成してもユーザーの許可設定は消えない
        upsert_generated(&conn, "p1", "c2", "h2", "i2").unwrap();
        assert!(get_context(&conn, "p1").unwrap().unwrap().allow_cloud_context);
    }

    #[test]
    fn test_update_cache_only_keeps_inventory_hash() {
        let conn = setup_db();
        create_project(&conn);
        upsert_generated(&conn, "p1", "c", "h", "ihash").unwrap();
        update_cache_only(&conn, "p1", "手編集後", "h2").unwrap();
        let ctx = get_context(&conn, "p1").unwrap().unwrap();
        assert_eq!(ctx.cached_context.as_deref(), Some("手編集後"));
        assert_eq!(ctx.inventory_hash.as_deref(), Some("ihash"), "inventory_hashは不変");
    }
}
