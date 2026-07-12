//! 添付ファイルのオンデマンド取得・保存コマンド。
//! 設計: docs/superpowers/specs/2026-07-12-attachment-download-design.md

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use tauri::State;

use crate::db::{accounts, attachments, mails};
use crate::error::AppError;
use crate::mail_sync::{imap_client, mime_parser};
use crate::models::attachment::Attachment;
use crate::state::{DbState, SecureStoreState};

/// 添付キャッシュのルート: {data_dir}/Pigeon/attachments
pub(crate) fn attachments_cache_root() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Pigeon")
        .join("attachments")
}

/// 添付ファイル名をキャッシュ保存用に正規化する。
/// - パス区切り（`/`, `\`）と NUL は `_` に置換
/// - 先頭のドットは除去（`..` などの相対パス表現・隠しファイル化を防ぐ）
/// - 空になった場合は `attachment-{n}`（n は 1 始まりの連番）
pub(crate) fn sanitize_filename(name: Option<&str>, index: usize) -> String {
    let cleaned: String = name
        .unwrap_or("")
        .chars()
        .map(|c| match c {
            '/' | '\\' | '\0' => '_',
            other => other,
        })
        .collect();
    let cleaned = cleaned.trim().trim_start_matches('.').to_string();
    if cleaned.is_empty() {
        format!("attachment-{}", index + 1)
    } else {
        cleaned
    }
}

/// attachments テーブルのレコードとキャッシュファイルが揃っていれば返す。
/// レコードが無い、または1つでもファイルが欠けていればキャッシュミス（None）。
pub(crate) fn load_cached_attachments(
    conn: &rusqlite::Connection,
    mail_id: &str,
) -> Result<Option<Vec<Attachment>>, AppError> {
    let records = attachments::get_by_mail_id(conn, mail_id)?;
    if records.is_empty() {
        return Ok(None);
    }
    let all_present = records.iter().all(|a| {
        a.file_path
            .as_deref()
            .is_some_and(|p| Path::new(p).is_file())
    });
    Ok(if all_present { Some(records) } else { None })
}

/// 元メールのバイト列から添付を抽出し、キャッシュ保存して attachments に全置換で記録する。
pub(crate) fn cache_attachments(
    conn: &rusqlite::Connection,
    cache_root: &Path,
    mail_id: &str,
    raw: &[u8],
) -> Result<Vec<Attachment>, AppError> {
    let extracted = mime_parser::extract_attachments(raw);

    let dir = cache_root.join(mail_id);
    std::fs::create_dir_all(&dir)
        .map_err(|e| AppError::FileIo(format!("Failed to create cache dir: {}", e)))?;

    attachments::delete_by_mail_id(conn, mail_id)?;

    let mut used_names: HashSet<String> = HashSet::new();
    let mut result = Vec::with_capacity(extracted.len());
    for (i, att) in extracted.iter().enumerate() {
        let mut filename = sanitize_filename(att.filename.as_deref(), i);
        // 同名添付の衝突は連番プレフィックスで回避する
        if !used_names.insert(filename.clone()) {
            filename = format!("{}-{}", i + 1, filename);
            used_names.insert(filename.clone());
        }

        let path = dir.join(&filename);
        std::fs::write(&path, &att.data)
            .map_err(|e| AppError::FileIo(format!("Failed to write cache file: {}", e)))?;

        result.push(attachments::insert_attachment(
            conn,
            mail_id,
            &filename,
            &att.mime_type,
            att.data.len() as i64,
            &path.to_string_lossy(),
            att.content_id.as_deref(),
        )?);
    }
    Ok(result)
}

/// キャッシュファイルを保存先へコピーする
pub(crate) fn copy_attachment_to(attachment: &Attachment, dest: &Path) -> Result<(), AppError> {
    let src = attachment
        .file_path
        .as_deref()
        .filter(|p| Path::new(p).is_file())
        .ok_or_else(|| AppError::AttachmentCacheMissing(attachment.filename.clone()))?;
    std::fs::copy(src, dest)
        .map_err(|e| AppError::FileIo(format!("Failed to save attachment: {}", e)))?;
    Ok(())
}

/// メールの添付一覧を返す。キャッシュが無ければ IMAP から元メールを取得して抽出する。
#[tauri::command]
pub async fn list_attachments(
    state: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    mail_id: String,
) -> Result<Vec<Attachment>, AppError> {
    // 1. メール情報の取得とキャッシュ確認（DBロック内・awaitなし）
    let (mail, account) = {
        let conn = state.0.lock().map_err(AppError::lock_err)?;
        let mail = mails::get_mail_by_id(&conn, &mail_id)?;
        if !mail.has_attachments {
            return Ok(Vec::new());
        }
        if let Some(cached) = load_cached_attachments(&conn, &mail_id)? {
            return Ok(cached);
        }
        let account = accounts::get_account(&conn, &mail.account_id)?;
        (mail, account)
    };

    // 2. IMAP から元メールを取得（DBロックは保持しない）
    let (auth_type, username, credential) =
        crate::commands::mail_commands::resolve_imap_credentials(&account, &secure_store.0).await?;
    let mut session = imap_client::connect(
        &account.imap_host,
        account.imap_port,
        &auth_type,
        &username,
        &credential,
    )
    .await?;
    let raw = imap_client::fetch_mail_raw(&mut session, &mail.folder, mail.uid).await;
    if let Err(e) = session.logout().await {
        eprintln!("[warn] IMAP logout failed: {}", e);
    }
    let raw = raw?;

    // 3. 抽出・キャッシュ保存・DB記録
    let conn = state.0.lock().map_err(AppError::lock_err)?;
    cache_attachments(&conn, &attachments_cache_root(), &mail_id, &raw)
}

/// キャッシュ済みの添付ファイルをユーザー指定のパスへコピーする
#[tauri::command]
pub fn save_attachment(
    state: State<'_, DbState>,
    attachment_id: String,
    dest_path: String,
) -> Result<(), AppError> {
    let attachment = {
        let conn = state.0.lock().map_err(AppError::lock_err)?;
        attachments::get_by_id(&conn, &attachment_id)?
    };
    copy_attachment_to(&attachment, Path::new(&dest_path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{insert_test_mail, setup_db};

    const MULTIPART_EMAIL: &[u8] = b"From: sender@example.com\r\n\
        To: recipient@example.com\r\n\
        Subject: With Attachments\r\n\
        Message-ID: <att@example.com>\r\n\
        Date: Sun, 12 Jul 2026 10:00:00 +0900\r\n\
        MIME-Version: 1.0\r\n\
        Content-Type: multipart/mixed; boundary=\"BOUNDARY\"\r\n\
        \r\n\
        --BOUNDARY\r\n\
        Content-Type: text/plain\r\n\
        \r\n\
        Please find attached.\r\n\
        --BOUNDARY\r\n\
        Content-Type: application/pdf; name=\"report.pdf\"\r\n\
        Content-Disposition: attachment; filename=\"report.pdf\"\r\n\
        Content-Transfer-Encoding: base64\r\n\
        \r\n\
        JVBERi0xLjQK\r\n\
        --BOUNDARY--\r\n";

    const EMAIL_WITH_INLINE_IMAGE: &[u8] = b"From: sender@example.com\r\n\
        To: recipient@example.com\r\n\
        Subject: Inline Image\r\n\
        Message-ID: <inline@example.com>\r\n\
        Date: Mon, 13 Jul 2026 10:00:00 +0900\r\n\
        MIME-Version: 1.0\r\n\
        Content-Type: multipart/related; boundary=\"BOUNDARY\"\r\n\
        \r\n\
        --BOUNDARY\r\n\
        Content-Type: text/html\r\n\
        \r\n\
        <html><body><img src=\"cid:logo123@example.com\"></body></html>\r\n\
        --BOUNDARY\r\n\
        Content-Type: image/png; name=\"logo.png\"\r\n\
        Content-Disposition: inline; filename=\"logo.png\"\r\n\
        Content-ID: <logo123@example.com>\r\n\
        Content-Transfer-Encoding: base64\r\n\
        \r\n\
        iVBORw0KGgo=\r\n\
        --BOUNDARY--\r\n";

    #[test]
    fn test_sanitize_filename_plain() {
        assert_eq!(sanitize_filename(Some("report.pdf"), 0), "report.pdf");
        assert_eq!(
            sanitize_filename(Some("日本語 資料.xlsx"), 0),
            "日本語 資料.xlsx"
        );
    }

    #[test]
    fn test_sanitize_filename_removes_path_separators() {
        assert_eq!(sanitize_filename(Some("a/b/c.txt"), 0), "a_b_c.txt");
        assert_eq!(sanitize_filename(Some("a\\b.txt"), 0), "a_b.txt");
        assert_eq!(sanitize_filename(Some("/etc/passwd"), 0), "_etc_passwd");
    }

    #[test]
    fn test_sanitize_filename_removes_dotdot() {
        assert_eq!(sanitize_filename(Some("../../evil.sh"), 0), "_.._evil.sh");
        assert_eq!(sanitize_filename(Some(".."), 0), "attachment-1");
        assert_eq!(sanitize_filename(Some(".hidden"), 0), "hidden");
    }

    #[test]
    fn test_sanitize_filename_empty_falls_back() {
        assert_eq!(sanitize_filename(None, 0), "attachment-1");
        assert_eq!(sanitize_filename(Some(""), 2), "attachment-3");
        assert_eq!(sanitize_filename(Some("   "), 1), "attachment-2");
    }

    #[test]
    fn test_sanitize_filename_replaces_nul() {
        assert_eq!(sanitize_filename(Some("a\0b.txt"), 0), "a_b.txt");
    }

    #[test]
    fn test_cache_attachments_writes_files_and_records() {
        let conn = setup_db();
        insert_test_mail(&conn, "m1", "With attachment");
        let tmp = tempfile::tempdir().unwrap();

        let result = cache_attachments(&conn, tmp.path(), "m1", MULTIPART_EMAIL).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].filename, "report.pdf");
        assert_eq!(result[0].mime_type, "application/pdf");
        assert_eq!(result[0].size, Some(9)); // "%PDF-1.4\n"

        let cached_path = tmp.path().join("m1").join("report.pdf");
        assert_eq!(std::fs::read(&cached_path).unwrap(), b"%PDF-1.4\n");
        assert_eq!(
            result[0].file_path.as_deref(),
            Some(cached_path.to_string_lossy().as_ref())
        );
        assert!(
            result[0].content_id.is_none(),
            "通常添付は content_id を持たない"
        );
    }

    #[test]
    fn test_cache_attachments_records_content_id_for_inline_image() {
        let conn = setup_db();
        insert_test_mail(&conn, "m1", "Inline image");
        let tmp = tempfile::tempdir().unwrap();

        let result = cache_attachments(&conn, tmp.path(), "m1", EMAIL_WITH_INLINE_IMAGE).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].filename, "logo.png");
        assert_eq!(result[0].content_id.as_deref(), Some("logo123@example.com"));
    }

    #[test]
    fn test_cache_attachments_replaces_previous_records() {
        let conn = setup_db();
        insert_test_mail(&conn, "m1", "With attachment");
        let tmp = tempfile::tempdir().unwrap();

        cache_attachments(&conn, tmp.path(), "m1", MULTIPART_EMAIL).unwrap();
        cache_attachments(&conn, tmp.path(), "m1", MULTIPART_EMAIL).unwrap();

        // 再取得してもレコードは全置換され重複しない
        let records = attachments::get_by_mail_id(&conn, "m1").unwrap();
        assert_eq!(records.len(), 1);
    }

    #[test]
    fn test_load_cached_attachments_hit() {
        let conn = setup_db();
        insert_test_mail(&conn, "m1", "With attachment");
        let tmp = tempfile::tempdir().unwrap();
        cache_attachments(&conn, tmp.path(), "m1", MULTIPART_EMAIL).unwrap();

        let cached = load_cached_attachments(&conn, "m1").unwrap();
        assert!(cached.is_some(), "レコードとファイルが揃っていればヒット");
        assert_eq!(cached.unwrap().len(), 1);
    }

    #[test]
    fn test_load_cached_attachments_miss_when_no_records() {
        let conn = setup_db();
        insert_test_mail(&conn, "m1", "No cache yet");
        assert!(load_cached_attachments(&conn, "m1").unwrap().is_none());
    }

    #[test]
    fn test_load_cached_attachments_miss_when_file_deleted() {
        let conn = setup_db();
        insert_test_mail(&conn, "m1", "With attachment");
        let tmp = tempfile::tempdir().unwrap();
        let result = cache_attachments(&conn, tmp.path(), "m1", MULTIPART_EMAIL).unwrap();

        // キャッシュファイルが消えたらミス扱い（再取得のトリガー）
        std::fs::remove_file(result[0].file_path.as_deref().unwrap()).unwrap();
        assert!(load_cached_attachments(&conn, "m1").unwrap().is_none());
    }

    #[test]
    fn test_copy_attachment_to_dest() {
        let conn = setup_db();
        insert_test_mail(&conn, "m1", "With attachment");
        let tmp = tempfile::tempdir().unwrap();
        let result = cache_attachments(&conn, tmp.path(), "m1", MULTIPART_EMAIL).unwrap();

        let dest = tmp.path().join("saved.pdf");
        copy_attachment_to(&result[0], &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"%PDF-1.4\n");
    }

    #[test]
    fn test_copy_attachment_errors_when_cache_missing() {
        let att = Attachment {
            id: "a1".into(),
            mail_id: "m1".into(),
            filename: "gone.pdf".into(),
            mime_type: "application/pdf".into(),
            size: Some(1),
            file_path: Some("/nonexistent/gone.pdf".into()),
            content_id: None,
        };
        let err = copy_attachment_to(&att, Path::new("/tmp/out.pdf")).unwrap_err();
        assert!(matches!(err, AppError::AttachmentCacheMissing(_)));
    }

    #[test]
    fn test_cache_attachments_handles_duplicate_names() {
        let conn = setup_db();
        insert_test_mail(&conn, "m1", "Duplicate names");
        let tmp = tempfile::tempdir().unwrap();

        const DUP_EMAIL: &[u8] = b"From: s@e.com\r\n\
            To: r@e.com\r\n\
            Subject: Dup\r\n\
            Message-ID: <dup@e.com>\r\n\
            Date: Sun, 12 Jul 2026 10:00:00 +0900\r\n\
            MIME-Version: 1.0\r\n\
            Content-Type: multipart/mixed; boundary=\"B\"\r\n\
            \r\n\
            --B\r\n\
            Content-Type: text/plain\r\n\
            \r\n\
            Body.\r\n\
            --B\r\n\
            Content-Type: text/plain; name=\"a.txt\"\r\n\
            Content-Disposition: attachment; filename=\"a.txt\"\r\n\
            \r\n\
            one\r\n\
            --B\r\n\
            Content-Type: text/plain; name=\"a.txt\"\r\n\
            Content-Disposition: attachment; filename=\"a.txt\"\r\n\
            \r\n\
            two\r\n\
            --B--\r\n";

        let result = cache_attachments(&conn, tmp.path(), "m1", DUP_EMAIL).unwrap();
        assert_eq!(result.len(), 2);
        let names: Vec<&str> = result.iter().map(|a| a.filename.as_str()).collect();
        assert!(names.contains(&"a.txt"));
        assert!(
            names.contains(&"2-a.txt"),
            "同名は連番プレフィックスで回避: {:?}",
            names
        );
    }
}
