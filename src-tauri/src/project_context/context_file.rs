use crate::error::AppError;
use std::path::Path;

pub const AUTO_MARKER: &str = "<!-- pigeon:auto -->";
pub const MAX_CACHED_CONTEXT_CHARS: usize = 800;
const FILE_NAME: &str = "PIGEON-CONTEXT.md";

/// マーカーで (ユーザー欄, auto部) に分割する。マーカー無しは (全文, None)。
/// 複数マーカーは最初のものを正とする（スペック§3 更新規約）。
pub fn split_at_marker(content: &str) -> (String, Option<String>) {
    match content.find(AUTO_MARKER) {
        Some(pos) => {
            let user = content[..pos].to_string();
            let auto = content[pos + AUTO_MARKER.len()..].to_string();
            (user, Some(auto))
        }
        None => (content.to_string(), None),
    }
}

/// auto セクションだけを差し替えた全文を返す。ユーザー欄（マーカーより上）は不可侵。
pub fn upsert_auto_section(existing: Option<&str>, project_name: &str, auto_body: &str) -> String {
    let user_section = match existing {
        Some(content) => split_at_marker(content).0,
        None => format!(
            "# {}\n\n（ここから上は自由記入欄です。Pigeon は書き換えません）\n\n",
            project_name
        ),
    };
    let user_trimmed = user_section.trim_end();
    format!(
        "{}\n\n{}\n{}\n",
        user_trimmed,
        AUTO_MARKER,
        auto_body.trim()
    )
}

/// 分類プロンプト注入用の切詰め。ユーザー欄を優先し、残り枠に auto を入れる。
pub fn build_cached_context(full_md: &str, max_chars: usize) -> String {
    let (user, auto) = split_at_marker(full_md);
    let user = user.trim();
    let auto = auto.unwrap_or_default();
    let auto = auto.trim();

    let user_chars: Vec<char> = user.chars().collect();
    if user_chars.len() >= max_chars {
        return user_chars[..max_chars].iter().collect();
    }
    let remaining = max_chars - user_chars.len() - 1; // 改行分
    let auto_part: String = auto.chars().take(remaining).collect();
    if auto_part.is_empty() {
        user.to_string()
    } else {
        format!("{}\n{}", user, auto_part)
    }
}

pub fn read_context_file(dir: &Path) -> Result<Option<String>, AppError> {
    let path = dir.join(FILE_NAME);
    match std::fs::read_to_string(&path) {
        Ok(content) => Ok(Some(content)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(AppError::DirectoryScan(format!(
            "{}: {}",
            path.display(),
            e
        ))),
    }
}

pub fn write_context_file(dir: &Path, content: &str) -> Result<(), AppError> {
    let path = dir.join(FILE_NAME);
    std::fs::write(&path, content)
        .map_err(|e| AppError::DirectoryScan(format!("{}: {}", path.display(), e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_upsert_creates_new_file_content() {
        let result = upsert_auto_section(None, "〇〇ホール 春公演", "- 会場: 〇〇ホール");
        assert!(result.starts_with("# 〇〇ホール 春公演"));
        assert!(result.contains(AUTO_MARKER));
        assert!(result.contains("- 会場: 〇〇ホール"));
        // マーカーはユーザー欄の後
        assert!(result.find(AUTO_MARKER).unwrap() > result.find("# 〇〇ホール").unwrap());
    }

    #[test]
    fn test_upsert_preserves_user_section() {
        let existing = format!(
            "# 手書きタイトル\n\n会場担当: 伊藤さん\n\n{}\n古い自動生成内容\n",
            AUTO_MARKER
        );
        let result = upsert_auto_section(Some(&existing), "ignored", "新しい内容");
        assert!(result.contains("# 手書きタイトル"));
        assert!(result.contains("会場担当: 伊藤さん"));
        assert!(result.contains("新しい内容"));
        assert!(!result.contains("古い自動生成内容"));
    }

    #[test]
    fn test_upsert_appends_marker_when_missing() {
        // ユーザーが自作したファイル（マーカー無し）→ 末尾に追加、本文は無傷
        let existing = "# 自作メモ\n大事なこと\n";
        let result = upsert_auto_section(Some(existing), "ignored", "auto内容");
        assert!(result.starts_with("# 自作メモ\n大事なこと\n"));
        assert!(result.contains(AUTO_MARKER));
        assert!(result.contains("auto内容"));
    }

    #[test]
    fn test_upsert_multiple_markers_first_wins() {
        let existing = format!("user部\n{}\n中身1\n{}\n中身2\n", AUTO_MARKER, AUTO_MARKER);
        let result = upsert_auto_section(Some(&existing), "ignored", "新");
        // 最初のマーカーを正とし、それ以降全体が auto セクションとして置換される
        assert_eq!(result.matches(AUTO_MARKER).count(), 1);
        assert!(!result.contains("中身1"));
        assert!(!result.contains("中身2"));
        assert!(result.contains("新"));
    }

    #[test]
    fn test_build_cached_context_prioritizes_user_section() {
        let user = "ユ".repeat(700);
        let auto = "オ".repeat(700);
        let md = format!("{}\n{}\n{}", user, AUTO_MARKER, auto);
        let cached = build_cached_context(&md, 800);
        assert!(cached.chars().count() <= 800);
        assert!(cached.contains(&"ユ".repeat(700)), "ユーザー欄は全量残る");
        assert!(cached.contains('オ'), "残り枠に auto が入る");
    }

    #[test]
    fn test_read_write_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_context_file(dir.path()).unwrap().is_none());
        write_context_file(dir.path(), "内容").unwrap();
        assert_eq!(read_context_file(dir.path()).unwrap().unwrap(), "内容");
    }
}
