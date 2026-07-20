//! 添付ファイルのオンデマンド取得・保存コマンド。
//! 設計: docs/archive/specs/2026-07-12-attachment-download-design.md

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;

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

/// メール削除後に添付キャッシュディレクトリ `{cache_root}/{mail_id}` を
/// ベストエフォートで削除する。キャッシュ掃除の失敗でメール削除自体を
/// 失敗させないため、エラーは警告ログのみで握りつぶす。
pub(crate) fn remove_attachment_cache(cache_root: &Path, mail_id: &str) {
    // mail_id は UUID の想定。万一パス区切り・`..`・絶対パス等を含む値が
    // 来ても cache_root の外を消さないよう、単一の通常コンポーネントのみ許可する
    let mut components = Path::new(mail_id).components();
    let is_single_normal = matches!(components.next(), Some(std::path::Component::Normal(_)))
        && components.next().is_none();
    if !is_single_normal {
        eprintln!(
            "[warn] Skipped attachment cache cleanup for unexpected mail_id: {:?}",
            mail_id
        );
        return;
    }
    match std::fs::remove_dir_all(cache_root.join(mail_id)) {
        Ok(()) => {}
        // キャッシュ未作成（一度も添付一覧を開いていない）は正常系
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => eprintln!(
            "[warn] Failed to remove attachment cache for {}: {}",
            mail_id, e
        ),
    }
}

/// 既定のキャッシュルート配下の添付キャッシュを削除する（削除系コマンド用）
pub(crate) fn remove_attachment_cache_for_mail(mail_id: &str) {
    remove_attachment_cache(&attachments_cache_root(), mail_id);
}

/// 保存先パスの防御的検証。保存先はバックエンドが開くダイアログで選ばれるため
/// 通常はすべて満たすが、書き込み直前の不変条件として明示的に検証する。
/// - 絶対パスであること（`..` / `.` セグメントを含まない）
/// - 親ディレクトリが実在すること
/// - 既存のディレクトリ・シンボリックリンクを上書きしないこと
pub(crate) fn validate_save_dest(dest: &Path) -> Result<(), AppError> {
    use std::path::Component;

    if !dest.is_absolute() {
        return Err(AppError::Validation(format!(
            "Save destination must be an absolute path: {}",
            dest.display()
        )));
    }
    if dest
        .components()
        .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err(AppError::Validation(format!(
            "Save destination must not contain '..' or '.' segments: {}",
            dest.display()
        )));
    }
    let parent = dest.parent().ok_or_else(|| {
        AppError::Validation(format!(
            "Save destination has no parent directory: {}",
            dest.display()
        ))
    })?;
    if !parent.is_dir() {
        return Err(AppError::Validation(format!(
            "Save destination directory does not exist: {}",
            parent.display()
        )));
    }
    // symlink_metadata はリンク自体を見る（存在しなければ Err → 新規作成なのでOK）
    if let Ok(meta) = std::fs::symlink_metadata(dest) {
        if meta.is_dir() {
            return Err(AppError::Validation(format!(
                "Save destination is a directory: {}",
                dest.display()
            )));
        }
        if meta.file_type().is_symlink() {
            return Err(AppError::Validation(format!(
                "Save destination is a symlink: {}",
                dest.display()
            )));
        }
    }
    Ok(())
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
        crate::commands::mail_commands::resolve_imap_credentials(&account, secure_store.get()?)
            .await?;
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
    state.with_conn(|conn| cache_attachments(conn, &attachments_cache_root(), &mail_id, &raw))
}

/// キャッシュ済みの添付ファイルを保存する。保存先はバックエンドで開く
/// ネイティブの保存ダイアログでユーザーが選択したパスに限定し、
/// IPC 境界からは保存先パスを受け取らない（任意パス書き込みの防止）。
/// 戻り値: 保存したら true、ユーザーがキャンセルしたら false。
#[tauri::command]
pub async fn save_attachment(
    app: tauri::AppHandle,
    state: State<'_, DbState>,
    attachment_id: String,
) -> Result<bool, AppError> {
    let attachment = state.with_conn(|conn| attachments::get_by_id(conn, &attachment_id))?;

    let mut dialog = app.dialog().file().set_file_name(&attachment.filename);
    if let Some(window) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&window);
    }
    let (tx, rx) = tokio::sync::oneshot::channel();
    dialog.save_file(move |picked| {
        // 受信側が先に破棄された場合のみ失敗するため、送信結果は無視してよい
        let _ = tx.send(picked);
    });
    let Some(picked) = rx
        .await
        .map_err(|e| AppError::FileIo(format!("Save dialog was closed unexpectedly: {}", e)))?
    else {
        return Ok(false); // ユーザーがキャンセル
    };
    let dest = picked
        .into_path()
        .map_err(|e| AppError::Validation(format!("Invalid save destination: {}", e)))?;
    validate_save_dest(&dest)?;
    copy_attachment_to(&attachment, &dest)?;
    Ok(true)
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

    // --- remove_attachment_cache (B-9: メール削除時のキャッシュ掃除) ---

    #[test]
    fn test_remove_attachment_cache_deletes_dir_recursively() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("m1");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.pdf"), b"x").unwrap();
        std::fs::write(dir.join("b.png"), b"y").unwrap();

        remove_attachment_cache(tmp.path(), "m1");
        assert!(!dir.exists());
        assert!(tmp.path().exists(), "キャッシュルート自体は残る");
    }

    #[test]
    fn test_remove_attachment_cache_ignores_missing_dir() {
        let tmp = tempfile::tempdir().unwrap();
        // キャッシュ未作成のメールでもパニック・エラーにならない
        remove_attachment_cache(tmp.path(), "never-cached");
        assert!(tmp.path().exists());
    }

    #[test]
    fn test_remove_attachment_cache_rejects_traversal() {
        let root = tempfile::tempdir().unwrap();
        let cache_root = root.path().join("attachments");
        std::fs::create_dir_all(&cache_root).unwrap();
        let victim = root.path().join("victim");
        std::fs::create_dir_all(&victim).unwrap();
        std::fs::write(victim.join("keep.txt"), b"x").unwrap();

        remove_attachment_cache(&cache_root, "../victim");
        assert!(
            victim.join("keep.txt").exists(),
            "cache_root の外は消さない"
        );
    }

    #[test]
    fn test_remove_attachment_cache_rejects_absolute_and_empty_mail_id() {
        let root = tempfile::tempdir().unwrap();
        let cache_root = root.path().join("attachments");
        std::fs::create_dir_all(&cache_root).unwrap();
        let victim = root.path().join("victim");
        std::fs::create_dir_all(&victim).unwrap();

        // 絶対パスは join でベースを置き換えてしまうため拒否する
        remove_attachment_cache(&cache_root, victim.to_str().unwrap());
        assert!(victim.exists());

        remove_attachment_cache(&cache_root, "");
        assert!(cache_root.exists());
    }

    // --- validate_save_dest (B-8: 保存先パスの防御的検証) ---

    #[test]
    fn test_validate_save_dest_accepts_new_file_in_existing_dir() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(validate_save_dest(&tmp.path().join("saved.pdf")).is_ok());
    }

    #[test]
    fn test_validate_save_dest_accepts_overwriting_regular_file() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("exists.pdf");
        std::fs::write(&dest, b"old").unwrap();
        assert!(
            validate_save_dest(&dest).is_ok(),
            "上書きはダイアログで確認済みのため許可"
        );
    }

    #[test]
    fn test_validate_save_dest_rejects_relative_path() {
        let err = validate_save_dest(Path::new("saved.pdf")).unwrap_err();
        assert!(matches!(err, AppError::Validation(_)));
    }

    #[test]
    fn test_validate_save_dest_rejects_parent_dir_component() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("..").join("evil.pdf");
        let err = validate_save_dest(&dest).unwrap_err();
        assert!(matches!(err, AppError::Validation(_)));
    }

    #[test]
    fn test_validate_save_dest_rejects_missing_parent_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("no-such-dir").join("saved.pdf");
        let err = validate_save_dest(&dest).unwrap_err();
        assert!(matches!(err, AppError::Validation(_)));
    }

    #[test]
    fn test_validate_save_dest_rejects_existing_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let err = validate_save_dest(tmp.path()).unwrap_err();
        assert!(matches!(err, AppError::Validation(_)));
    }

    #[cfg(unix)]
    #[test]
    fn test_validate_save_dest_rejects_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("target.pdf");
        std::fs::write(&target, b"x").unwrap();
        let link = tmp.path().join("link.pdf");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let err = validate_save_dest(&link).unwrap_err();
        assert!(matches!(err, AppError::Validation(_)));
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
