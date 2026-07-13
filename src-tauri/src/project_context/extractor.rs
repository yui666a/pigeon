use crate::error::AppError;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;

pub const MAX_EXTRACT_BYTES_PER_FILE: usize = 10 * 1024;
pub const MAX_EXTRACT_BYTES_PER_PROJECT: usize = 100 * 1024;
pub const MAX_HASHABLE_FILE_BYTES: u64 = 1024 * 1024;

const TEXT_EXTENSIONS: &[&str] = &["txt", "md", "csv", "json", "yaml", "yml", "html"];
const OFFICE_EXTENSIONS: &[&str] = &["xlsx", "xls", "docx", "doc", "pptx", "ppt"];

pub struct ExtractedText {
    pub text: String,
    pub truncated: bool,
    pub hash: String,
}

pub fn content_kind_for(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();
    if TEXT_EXTENSIONS.contains(&ext.as_str()) {
        "text"
    } else if ext == "pdf" {
        "pdf"
    } else if OFFICE_EXTENSIONS.contains(&ext.as_str()) {
        "office"
    } else {
        "other"
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{:02x}", b)).collect()
}

/// テキストファイルの内容を上限付きで読む。ハッシュは抽出（切詰後）バイト列に対して計算する。
pub fn extract_text(path: &Path) -> Result<ExtractedText, AppError> {
    let file = std::fs::File::open(path)
        .map_err(|e| AppError::DirectoryScan(format!("{}: {}", path.display(), e)))?;
    let mut buf = Vec::with_capacity(MAX_EXTRACT_BYTES_PER_FILE);
    let mut handle = file.take((MAX_EXTRACT_BYTES_PER_FILE + 1) as u64);
    handle
        .read_to_end(&mut buf)
        .map_err(|e| AppError::DirectoryScan(format!("{}: {}", path.display(), e)))?;

    let truncated = buf.len() > MAX_EXTRACT_BYTES_PER_FILE;
    buf.truncate(MAX_EXTRACT_BYTES_PER_FILE);
    let hash = sha256_hex(&buf);
    let text = String::from_utf8_lossy(&buf).into_owned();
    Ok(ExtractedText {
        text,
        truncated,
        hash,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_content_kind_for() {
        assert_eq!(content_kind_for(Path::new("a.txt")), "text");
        assert_eq!(content_kind_for(Path::new("香盤表.md")), "text");
        assert_eq!(content_kind_for(Path::new("data.CSV")), "text"); // 大文字拡張子
        assert_eq!(content_kind_for(Path::new("平面図.pdf")), "pdf");
        assert_eq!(content_kind_for(Path::new("見積.xlsx")), "office");
        assert_eq!(content_kind_for(Path::new("photo.jpg")), "other");
        assert_eq!(content_kind_for(Path::new("no_extension")), "other");
    }

    #[test]
    fn test_extract_text_small_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("memo.txt");
        std::fs::write(&path, "搬入は9時から").unwrap();

        let extracted = extract_text(&path).unwrap();
        assert_eq!(extracted.text, "搬入は9時から");
        assert!(!extracted.truncated);
        assert_eq!(extracted.hash.len(), 64); // sha256 hex
    }

    #[test]
    fn test_extract_text_truncates_at_cap() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.txt");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(&vec![b'a'; MAX_EXTRACT_BYTES_PER_FILE + 500])
            .unwrap();

        let extracted = extract_text(&path).unwrap();
        assert!(extracted.truncated);
        assert!(extracted.text.len() <= MAX_EXTRACT_BYTES_PER_FILE);
    }

    #[test]
    fn test_extract_text_same_content_same_hash() {
        let dir = tempfile::tempdir().unwrap();
        let p1 = dir.path().join("a.txt");
        let p2 = dir.path().join("b.txt");
        std::fs::write(&p1, "same").unwrap();
        std::fs::write(&p2, "same").unwrap();
        assert_eq!(
            extract_text(&p1).unwrap().hash,
            extract_text(&p2).unwrap().hash
        );
    }
}
