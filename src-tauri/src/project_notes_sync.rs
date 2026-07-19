use crate::db::{directories, project_contexts, project_notes};
use crate::error::AppError;
use crate::project_context::context_file::{
    build_cached_context, read_context_file, split_at_marker, upsert_auto_section,
    write_context_file, MAX_CACHED_CONTEXT_CHARS,
};
use crate::project_context::extractor::sha256_hex;
use rusqlite::Connection;
use std::path::Path;

/// user_md + ai_md を1本の PIGEON-CONTEXT.md 形式へ合成する。
/// 既存の upsert_auto_section の規約（マーカーより上がユーザー欄）に合わせる。
pub fn compose_markdown(user_md: &str, ai_md: Option<&str>, project_name: &str) -> String {
    let existing_user = if user_md.trim().is_empty() {
        None
    } else {
        Some(user_md)
    };
    upsert_auto_section(existing_user, project_name, ai_md.unwrap_or(""))
}

/// PIGEON-CONTEXT.md 形式を (user_md, ai_md) へ分解する。
/// マーカー無しのファイル（ユーザーの自作）は全体をユーザー欄として扱う。
pub fn decompose_markdown(full_md: &str) -> (String, Option<String>) {
    split_at_marker(full_md)
}

/// 案件ノートをディレクトリの PIGEON-CONTEXT.md へ書き出す（DB→ファイルのミラー）。
/// ディレクトリ未連携の案件では何もしない。
pub fn sync_note_to_disk(conn: &Connection, project_id: &str) -> Result<(), AppError> {
    let dir = match directories::get_directory_by_project(conn, project_id)? {
        Some(d) => d,
        None => return Ok(()),
    };
    let note = match project_notes::get_note(conn, project_id)? {
        Some(n) => n,
        None => return Ok(()),
    };
    let project_name = crate::db::projects::get_project(conn, project_id)?.name;
    let composed = compose_markdown(&note.user_md, note.ai_md.as_deref(), &project_name);
    write_context_file(Path::new(&dir.path), &composed)
}

/// 案件ノートから分類プロンプト注入用キャッシュ (project_contexts.cached_context) を再生成する。
pub fn refresh_cached_context(conn: &Connection, project_id: &str) -> Result<(), AppError> {
    let note = match project_notes::get_note(conn, project_id)? {
        Some(n) => n,
        None => return Ok(()),
    };
    let project_name = crate::db::projects::get_project(conn, project_id)?.name;
    let composed = compose_markdown(&note.user_md, note.ai_md.as_deref(), &project_name);
    let cached = build_cached_context(&composed, MAX_CACHED_CONTEXT_CHARS);
    let hash = sha256_hex(composed.as_bytes());
    project_contexts::update_cache_only(conn, project_id, &cached, &hash)
}

/// ディレクトリ上の PIGEON-CONTEXT.md をDBへ取り込む（ファイル→DB、自己修復・初期移行用）。
/// 外部エディタでの編集を DB 正本へ反映する。
pub fn import_note_from_disk(conn: &Connection, project_id: &str) -> Result<bool, AppError> {
    let dir = match directories::get_directory_by_project(conn, project_id)? {
        Some(d) => d,
        None => return Ok(false),
    };
    let full_md = match read_context_file(Path::new(&dir.path))? {
        Some(md) => md,
        None => return Ok(false),
    };
    let (user_md, ai_md) = decompose_markdown(&full_md);
    project_notes::upsert_user_md(conn, project_id, user_md.trim())?;
    if let Some(ai) = ai_md {
        // ファイル由来の取り込みは「AI生成そのまま」とみなし edited は立てない
        project_notes::upsert_ai_md(conn, project_id, ai.trim(), false)?;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::setup_db;

    fn create_project(conn: &Connection) {
        conn.execute(
            "INSERT INTO projects (id, account_id, name) VALUES ('p1', 'acc1', '春公演')",
            [],
        )
        .unwrap();
    }

    #[test]
    fn test_compose_then_decompose_roundtrip() {
        let composed =
            compose_markdown("# 手書き\n会場担当: 伊藤", Some("- 公演: 春公演"), "春公演");
        let (user, ai) = decompose_markdown(&composed);
        assert!(user.contains("# 手書き"));
        assert!(user.contains("会場担当: 伊藤"));
        assert_eq!(ai.as_deref().map(str::trim), Some("- 公演: 春公演"));
    }

    #[test]
    fn test_compose_without_ai_section() {
        let composed = compose_markdown("手書きのみ", None, "春公演");
        let (user, _ai) = decompose_markdown(&composed);
        assert!(user.contains("手書きのみ"));
    }

    #[test]
    fn test_decompose_file_without_marker_is_all_user() {
        // ユーザーが外部エディタで自作した（マーカー無し）ファイル
        let (user, ai) = decompose_markdown("# 自作メモ\n大事なこと\n");
        assert!(user.contains("# 自作メモ"));
        assert_eq!(ai, None, "マーカー無しは全部ユーザー欄");
    }

    #[test]
    fn test_sync_to_disk_writes_file_when_linked() {
        let mut conn = setup_db();
        create_project(&conn);
        let dir = tempfile::tempdir().unwrap();
        directories::link_directory(&mut conn, "p1", dir.path().to_str().unwrap()).unwrap();

        project_notes::upsert_user_md(&conn, "p1", "会場担当: 伊藤").unwrap();
        project_notes::upsert_ai_md(&conn, "p1", "- 公演: 春公演", false).unwrap();

        sync_note_to_disk(&conn, "p1").unwrap();

        let written = std::fs::read_to_string(dir.path().join("PIGEON-CONTEXT.md")).unwrap();
        assert!(written.contains("会場担当: 伊藤"));
        assert!(written.contains("- 公演: 春公演"));
    }

    #[test]
    fn test_sync_to_disk_noop_when_not_linked() {
        let conn = setup_db();
        create_project(&conn);
        project_notes::upsert_user_md(&conn, "p1", "ノート").unwrap();
        // ディレクトリ未連携でもエラーにならない（何もしない）
        sync_note_to_disk(&conn, "p1").unwrap();
    }

    #[test]
    fn test_refresh_cached_context_prioritizes_user_section() {
        let conn = setup_db();
        create_project(&conn);
        project_notes::upsert_user_md(&conn, "p1", "ユーザー欄の内容").unwrap();
        project_notes::upsert_ai_md(&conn, "p1", "AI欄の内容", false).unwrap();

        refresh_cached_context(&conn, "p1").unwrap();

        let ctx = project_contexts::get_context(&conn, "p1").unwrap().unwrap();
        let cached = ctx.cached_context.unwrap();
        assert!(cached.contains("ユーザー欄の内容"));
        assert!(cached.chars().count() <= MAX_CACHED_CONTEXT_CHARS);
    }

    #[test]
    fn test_import_from_disk_splits_into_two_columns() {
        let mut conn = setup_db();
        create_project(&conn);
        let dir = tempfile::tempdir().unwrap();
        directories::link_directory(&mut conn, "p1", dir.path().to_str().unwrap()).unwrap();

        let file_content = format!(
            "# 手書きタイトル\n担当: 伊藤\n\n{}\n- 公演: 春公演\n",
            crate::project_context::context_file::AUTO_MARKER
        );
        std::fs::write(dir.path().join("PIGEON-CONTEXT.md"), &file_content).unwrap();

        assert!(import_note_from_disk(&conn, "p1").unwrap());

        let note = project_notes::get_note(&conn, "p1").unwrap().unwrap();
        assert!(note.user_md.contains("担当: 伊藤"));
        assert_eq!(note.ai_md.as_deref(), Some("- 公演: 春公演"));
        assert!(!note.ai_edited);
    }
}
