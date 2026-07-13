//! インライン画像（cid:）の本文内表示のためのデータ供給コマンド。
//! 添付ダウンロード（attachment_commands.rs）のオンデマンド取得＋キャッシュ経路を
//! そのまま流用し、content_id を持つ添付だけを data URI にして返す。
//! 設計: docs/superpowers/specs/2026-07-13-inline-cid-images-design.md

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::Serialize;
use tauri::State;

use crate::commands::attachment_commands::{
    attachments_cache_root, cache_attachments, load_cached_attachments,
};
use crate::db::{accounts, mails};
use crate::error::AppError;
use crate::mail_sync::imap_client;
use crate::models::attachment::Attachment;
use crate::state::{DbState, SecureStoreState};

/// 本文中の `<img src="cid:...">` に対応する画像データ（data URI）
#[derive(Debug, Clone, Serialize)]
pub struct InlineImage {
    pub content_id: String,
    pub data_uri: String,
}

/// キャッシュ済み添付から content_id を持つものだけを data URI に変換する純関数
fn to_inline_images(attachments: &[Attachment]) -> Vec<InlineImage> {
    attachments
        .iter()
        .filter_map(|a| {
            let content_id = a.content_id.clone()?;
            let path = a.file_path.as_deref()?;
            let data = std::fs::read(path).ok()?;
            Some(InlineImage {
                content_id,
                data_uri: format!("data:{};base64,{}", a.mime_type, STANDARD.encode(data)),
            })
        })
        .collect()
}

/// メール本文中の cid 参照に対応する画像を返す。
/// 添付一覧と同じキャッシュ経路（list_attachments 相当）を使うため、
/// 未キャッシュなら IMAP から元メールを取得する。
#[tauri::command]
pub async fn get_inline_images(
    state: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    mail_id: String,
) -> Result<Vec<InlineImage>, AppError> {
    let (mail, account) = {
        let conn = state.0.lock().map_err(AppError::lock_err)?;
        let mail = mails::get_mail_by_id(&conn, &mail_id)?;
        if !mail.has_attachments {
            return Ok(Vec::new());
        }
        if let Some(cached) = load_cached_attachments(&conn, &mail_id)? {
            return Ok(to_inline_images(&cached));
        }
        let account = accounts::get_account(&conn, &mail.account_id)?;
        (mail, account)
    };

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

    let attachments = state
        .with_conn(|conn| cache_attachments(conn, &attachments_cache_root(), &mail_id, &raw))?;
    Ok(to_inline_images(&attachments))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_inline_images_filters_non_inline_attachments() {
        let attachments = vec![
            Attachment {
                id: "a1".into(),
                mail_id: "m1".into(),
                filename: "report.pdf".into(),
                mime_type: "application/pdf".into(),
                size: Some(3),
                file_path: None,
                content_id: None,
            },
            Attachment {
                id: "a2".into(),
                mail_id: "m1".into(),
                filename: "logo.png".into(),
                mime_type: "image/png".into(),
                size: Some(4),
                file_path: Some("/nonexistent/logo.png".into()),
                content_id: Some("logo123@example.com".into()),
            },
        ];
        // content_id はあるがキャッシュファイルが存在しない場合は除外される
        let images = to_inline_images(&attachments);
        assert!(images.is_empty());
    }

    #[test]
    fn test_to_inline_images_builds_data_uri() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("logo.png");
        std::fs::write(&path, b"\x89PNG\r\n\x1a\n").unwrap();

        let attachments = vec![Attachment {
            id: "a1".into(),
            mail_id: "m1".into(),
            filename: "logo.png".into(),
            mime_type: "image/png".into(),
            size: Some(8),
            file_path: Some(path.to_string_lossy().to_string()),
            content_id: Some("logo123@example.com".into()),
        }];

        let images = to_inline_images(&attachments);
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].content_id, "logo123@example.com");
        assert!(images[0].data_uri.starts_with("data:image/png;base64,"));
    }
}
