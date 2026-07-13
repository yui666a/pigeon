use crate::error::AppError;
use crate::models::directory::ProjectFileEntry;
use crate::project_context::extractor;
use chrono::{DateTime, Utc};
use std::path::Path;

pub const CONTEXT_FILE_NAME: &str = "PIGEON-CONTEXT.md";
pub const MAX_FILES: usize = 2000;
pub const MAX_DEPTH: usize = 10;
const IGNORED_DIRS: &[&str] = &["node_modules", "target", ".git"];

pub struct ScanResult {
    pub files: Vec<ProjectFileEntry>,
    pub inventory_hash: String,
}

pub fn classify_io_error(e: &std::io::Error) -> &'static str {
    match e.kind() {
        std::io::ErrorKind::NotFound => "missing",
        std::io::ErrorKind::PermissionDenied => "inaccessible",
        _ => "error",
    }
}

pub fn scan_directory(root: &Path) -> Result<ScanResult, AppError> {
    // ルートの存在確認(io::Error を文言に含め、呼び出し側が classify できるようにする)
    std::fs::read_dir(root)
        .map_err(|e| AppError::DirectoryScan(format!("{} [{}]", e, classify_io_error(&e))))?;

    let mut files = Vec::new();
    walk(root, root, 0, &mut files)?;
    files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    files.truncate(MAX_FILES);

    let mut hash_input = String::new();
    for f in &files {
        hash_input.push_str(&format!(
            "{}|{}|{}|{}\n",
            f.relative_path,
            f.size_bytes,
            f.mtime,
            f.content_hash.as_deref().unwrap_or("-")
        ));
    }
    let inventory_hash = extractor::sha256_hex(hash_input.as_bytes());

    Ok(ScanResult {
        files,
        inventory_hash,
    })
}

fn walk(
    root: &Path,
    current: &Path,
    depth: usize,
    out: &mut Vec<ProjectFileEntry>,
) -> Result<(), AppError> {
    if depth > MAX_DEPTH || out.len() >= MAX_FILES {
        return Ok(());
    }
    let entries = match std::fs::read_dir(current) {
        Ok(e) => e,
        Err(_) => return Ok(()), // サブディレクトリの読み取り失敗はスキップして続行
    };
    for entry in entries.flatten() {
        if out.len() >= MAX_FILES {
            return Ok(());
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue; // 隠しファイル・隠しディレクトリ
        }
        let file_type = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        if file_type.is_symlink() {
            continue; // ループ・案件外脱出の防止
        }
        let path = entry.path();
        if file_type.is_dir() {
            if IGNORED_DIRS.contains(&name.as_str()) {
                continue;
            }
            walk(root, &path, depth + 1, out)?;
            continue;
        }
        if depth == 0 && name == CONTEXT_FILE_NAME {
            continue; // 自己参照ループ防止(スペック§4)
        }
        if let Some(file_entry) = build_entry(root, &path) {
            out.push(file_entry);
        }
    }
    Ok(())
}

fn build_entry(root: &Path, path: &Path) -> Option<ProjectFileEntry> {
    let meta = std::fs::metadata(path).ok()?;
    let relative_path = path.strip_prefix(root).ok()?.to_string_lossy().into_owned();
    let mtime: DateTime<Utc> = meta.modified().ok()?.into();
    let content_kind = extractor::content_kind_for(path);

    let (content_hash, extract_status) = if content_kind == "text" {
        if meta.len() > extractor::MAX_HASHABLE_FILE_BYTES {
            (None, "skipped_too_large")
        } else {
            match extractor::extract_text(path) {
                Ok(e) => (Some(e.hash), "ok"),
                Err(_) => (None, "error"),
            }
        }
    } else {
        (None, "unsupported")
    };

    Some(ProjectFileEntry {
        relative_path,
        size_bytes: meta.len() as i64,
        mtime: mtime.to_rfc3339(),
        content_hash,
        content_kind: content_kind.to_string(),
        extract_status: extract_status.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tree(dir: &std::path::Path) {
        std::fs::create_dir_all(dir.join("図面")).unwrap();
        std::fs::write(dir.join("図面/平面図.pdf"), b"%PDF-fake").unwrap();
        std::fs::write(dir.join("香盤表.md"), "第1幕 くるみ割り").unwrap();
        std::fs::write(dir.join("搬入.txt"), "9時集合").unwrap();
    }

    #[test]
    fn test_scan_directory_basic() {
        let dir = tempfile::tempdir().unwrap();
        make_tree(dir.path());

        let result = scan_directory(dir.path()).unwrap();
        let paths: Vec<&str> = result
            .files
            .iter()
            .map(|f| f.relative_path.as_str())
            .collect();
        assert_eq!(paths, vec!["図面/平面図.pdf", "搬入.txt", "香盤表.md"]);

        let md = result
            .files
            .iter()
            .find(|f| f.relative_path == "香盤表.md")
            .unwrap();
        assert_eq!(md.content_kind, "text");
        assert_eq!(md.extract_status, "ok");
        assert!(md.content_hash.is_some());

        let pdf = result
            .files
            .iter()
            .find(|f| f.relative_path.ends_with("平面図.pdf"))
            .unwrap();
        assert_eq!(pdf.content_kind, "pdf");
        assert_eq!(pdf.extract_status, "unsupported");
        assert!(pdf.content_hash.is_none());
    }

    #[test]
    fn test_scan_skips_hidden_symlink_node_modules_and_context_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".DS_Store"), b"x").unwrap();
        std::fs::create_dir_all(dir.path().join("node_modules/pkg")).unwrap();
        std::fs::write(dir.path().join("node_modules/pkg/index.js"), b"x").unwrap();
        std::fs::write(dir.path().join(CONTEXT_FILE_NAME), "# ctx").unwrap();
        std::fs::write(dir.path().join("keep.txt"), "keep").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(dir.path().join("keep.txt"), dir.path().join("link.txt"))
            .unwrap();

        let result = scan_directory(dir.path()).unwrap();
        let paths: Vec<&str> = result
            .files
            .iter()
            .map(|f| f.relative_path.as_str())
            .collect();
        assert_eq!(paths, vec!["keep.txt"]);
    }

    #[test]
    fn test_inventory_hash_stable_and_change_sensitive() {
        let dir = tempfile::tempdir().unwrap();
        make_tree(dir.path());

        let h1 = scan_directory(dir.path()).unwrap().inventory_hash;
        let h2 = scan_directory(dir.path()).unwrap().inventory_hash;
        assert_eq!(h1, h2, "同一構成なら同一ハッシュ");

        std::fs::write(dir.path().join("新資料.txt"), "追加").unwrap();
        let h3 = scan_directory(dir.path()).unwrap().inventory_hash;
        assert_ne!(h1, h3, "ファイル追加でハッシュが変わる");
    }

    #[test]
    fn test_scan_missing_root_is_error() {
        let result = scan_directory(std::path::Path::new("/nonexistent/pigeon-test"));
        assert!(result.is_err());
    }

    #[test]
    fn test_classify_io_error() {
        use std::io::{Error, ErrorKind};
        assert_eq!(
            classify_io_error(&Error::from(ErrorKind::NotFound)),
            "missing"
        );
        assert_eq!(
            classify_io_error(&Error::from(ErrorKind::PermissionDenied)),
            "inaccessible"
        );
        assert_eq!(classify_io_error(&Error::from(ErrorKind::Other)), "error");
    }
}
