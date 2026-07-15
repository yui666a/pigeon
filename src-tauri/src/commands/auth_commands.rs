use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

use crate::db::accounts;
use crate::error::AppError;
use crate::mail_sync::oauth::{self, OAuthConfig, OAuthStateStore, PendingOAuth};
use crate::models::account::{AccountProvider, AuthType, CreateAccountRequest, OAuthTokenData};
use crate::secure_store::SecureStore;
use crate::state::{DbState, SecureStoreState};

const LOOPBACK_HOST: &str = "127.0.0.1";
const OAUTH_CALLBACK_PATH: &str = "/oauth/callback";

#[tauri::command]
pub async fn start_oauth(
    app_handle: AppHandle,
    oauth_store: State<'_, OAuthStateStore>,
    provider: String,
    account_id: Option<String>,
) -> Result<String, AppError> {
    start_oauth_inner(&app_handle, &oauth_store, &provider, account_id)
}

fn start_oauth_inner(
    app_handle: &AppHandle,
    oauth_store: &OAuthStateStore,
    provider: &str,
    existing_account_id: Option<String>,
) -> Result<String, AppError> {
    match provider {
        "google" => {
            let redirect_uri = start_loopback_callback_listener(app_handle.clone())?;
            let config = OAuthConfig::google_with_redirect(redirect_uri.clone())?;
            let account_id = existing_account_id.unwrap_or_else(|| Uuid::new_v4().to_string());
            let code_verifier = oauth::generate_code_verifier();
            let code_challenge = oauth::generate_code_challenge(&code_verifier);
            let state = oauth::generate_state();

            let now = oauth::current_timestamp();

            oauth_store.store(
                state.clone(),
                PendingOAuth {
                    account_id,
                    code_verifier,
                    redirect_uri,
                    created_at: now,
                },
            );

            oauth_store.cleanup_expired();

            let auth_url = oauth::build_auth_url(&config, &state, &code_challenge);
            Ok(auth_url)
        }
        _ => Err(AppError::OAuth(format!(
            "Unsupported OAuth provider: {}",
            provider
        ))),
    }
}

fn start_loopback_callback_listener(app_handle: AppHandle) -> Result<String, AppError> {
    let listener = TcpListener::bind((LOOPBACK_HOST, 0))
        .map_err(|e| AppError::OAuth(format!("Failed to bind OAuth loopback listener: {}", e)))?;
    let port = listener
        .local_addr()
        .map_err(|e| AppError::OAuth(format!("Failed to read loopback listener port: {}", e)))?
        .port();

    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            handle_loopback_request(&app_handle, port, &mut stream);
        }
    });

    Ok(format!(
        "http://{}:{}{}",
        LOOPBACK_HOST, port, OAUTH_CALLBACK_PATH
    ))
}

/// リクエストライン読み取りの上限バイト数。OAuth コールバックの
/// リクエストラインはこの範囲に必ず収まる（メモリ膨張ガード）
const MAX_REQUEST_LINE_BYTES: usize = 8192;

fn handle_loopback_request(app_handle: &AppHandle, port: u16, stream: &mut TcpStream) {
    // 何も送ってこないクライアントでリスナースレッドが永久ブロックしないように
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(10)));
    let request_line = read_request_line(stream);
    let request_target = request_line.as_deref().and_then(parse_request_target);

    let (status, body) = match request_target {
        Some(target) if target.starts_with(OAUTH_CALLBACK_PATH) => {
            let callback_url = format!("http://{}:{}{}", LOOPBACK_HOST, port, target);
            let _ = app_handle.emit("deep-link://new-url", vec![callback_url]);
            (
                "200 OK",
                "<html><body><h2>OAuth completed. You can close this tab.</h2></body></html>",
            )
        }
        _ => (
            "400 Bad Request",
            "<html><body><h2>Invalid OAuth callback request.</h2></body></html>",
        ),
    };

    let response = format!(
        "HTTP/1.1 {}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status,
        body.len(),
        body
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

/// HTTP リクエストの先頭行（リクエストライン）を read ループで読み取る。
/// TCP ではリクエストラインが複数の read に分割されて届き得るため、
/// 改行（`\n`）が現れるまで読み足す。固定長バッファへの単一 read だと
/// 分割到着時にコールバックを取りこぼす。
///
/// - 改行前に EOF に達した場合は読めた分をそのまま返す
/// - 改行が `MAX_REQUEST_LINE_BYTES` を超えても現れない場合は打ち切って None
fn read_request_line(stream: &mut impl Read) -> Option<String> {
    let mut buf: Vec<u8> = Vec::with_capacity(1024);
    let mut chunk = [0_u8; 1024];
    loop {
        if let Some(pos) = buf.iter().position(|&b| b == b'\n') {
            let line = String::from_utf8_lossy(&buf[..pos]);
            return Some(line.trim_end_matches('\r').to_string());
        }
        if buf.len() >= MAX_REQUEST_LINE_BYTES {
            return None;
        }
        match stream.read(&mut chunk) {
            Ok(0) => {
                // EOF: 改行なしで接続が閉じられた。読めた分があればそれを行として扱う
                if buf.is_empty() {
                    return None;
                }
                let line = String::from_utf8_lossy(&buf);
                return Some(line.trim_end_matches('\r').to_string());
            }
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return None,
        }
    }
}

fn parse_request_target(request: &str) -> Option<&str> {
    let first_line = request.lines().next()?;
    let mut parts = first_line.split_whitespace();
    let method = parts.next()?;
    if method != "GET" {
        return None;
    }
    parts.next()
}

#[tauri::command]
pub async fn handle_oauth_callback(
    app: AppHandle,
    state: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    oauth_store: State<'_, OAuthStateStore>,
    url: String,
) -> Result<String, AppError> {
    let account_id =
        handle_oauth_callback_inner(&state, &secure_store.0, &oauth_store, &url).await?;
    // OAuth 完了（新規追加・再認証とも）したアカウントの IDLE 監視を開始する。
    // 再認証の場合は停止していた監視タスクがここで置き換え再開される
    crate::mail_sync::idle::start_watching(&app, &account_id);
    Ok(account_id)
}

async fn handle_oauth_callback_inner(
    db_state: &DbState,
    secure_store: &SecureStore,
    oauth_store: &OAuthStateStore,
    url: &str,
) -> Result<String, AppError> {
    let (code, state_param) = oauth::parse_callback_url(url)?;

    let pending = oauth_store
        .take(&state_param)
        .ok_or(AppError::InvalidOAuthState)?;

    // Check if the pending OAuth entry has expired
    if oauth::is_pending_expired(pending.created_at, oauth::current_timestamp()) {
        return Err(AppError::OAuthTimeout);
    }

    let config = OAuthConfig::google_with_redirect(pending.redirect_uri.clone())?;

    // Exchange authorization code for tokens
    let token_response = oauth::exchange_code(&config, &code, &pending.code_verifier).await?;

    // Extract email from ID token
    let email = match &token_response.id_token {
        Some(id_token) => oauth::decode_id_token_email(id_token, &config.client_id)?,
        None => return Err(AppError::OAuth("No ID token in response".into())),
    };

    // Build token data
    let token_data = oauth::build_token_data(&token_response, &email, None)?;

    // 再認証判定 → 重複メール判定 → アカウント挿入は、間に別コマンドの書き込みが
    // 割り込むと判定が古くなる（TOCTOU）ため、単一のロックスコープで行う
    db_state.with_conn(|conn| {
        // Check if this is a reauth (account already exists in DB)
        if accounts::get_account(conn, &pending.account_id).is_ok() {
            // Reauth: only save token, skip DB insert
            save_oauth_token(secure_store, &pending.account_id, &token_data)?;
            return Ok(pending.account_id.clone());
        }

        // Check for duplicate email
        if let Some(existing) = accounts::account_exists_by_email(conn, &email)? {
            return Err(AppError::DuplicateAccount(format!(
                "Account with email {} already exists (id: {})",
                email, existing.id
            )));
        }

        // Save tokens to SecureStore
        save_oauth_token(secure_store, &pending.account_id, &token_data)?;

        // Save account to DB
        let req = CreateAccountRequest {
            name: email.clone(),
            email: email.clone(),
            imap_host: oauth::GOOGLE_IMAP_HOST.into(),
            imap_port: oauth::GOOGLE_IMAP_PORT,
            smtp_host: oauth::GOOGLE_SMTP_HOST.into(),
            smtp_port: oauth::GOOGLE_SMTP_PORT,
            auth_type: AuthType::Oauth2,
            provider: AccountProvider::Google,
            password: None,
        };
        match accounts::insert_account_with_id(conn, &pending.account_id, &req) {
            Ok(account) => Ok(account.id),
            Err(e) => {
                // Compensating action: remove token from SecureStore if DB insert fails
                let _ = secure_store.delete(&format!("oauth_{}", pending.account_id));
                Err(e)
            }
        }
    })
}

pub fn save_oauth_token(
    secure_store: &SecureStore,
    account_id: &str,
    token_data: &OAuthTokenData,
) -> Result<(), AppError> {
    let key = format!("oauth_{}", account_id);
    let value = serde_json::to_vec(token_data)
        .map_err(|e| AppError::Stronghold(format!("Failed to serialize token data: {}", e)))?;
    secure_store.insert(&key, &value)
}

pub fn load_oauth_token(
    secure_store: &SecureStore,
    account_id: &str,
) -> Result<OAuthTokenData, AppError> {
    let key = format!("oauth_{}", account_id);
    let value = secure_store.get(&key)?.ok_or_else(|| {
        AppError::Stronghold(format!("No OAuth token found for account {}", account_id))
    })?;
    let token_data: OAuthTokenData = serde_json::from_slice(&value)
        .map_err(|e| AppError::Stronghold(format!("Failed to deserialize token data: {}", e)))?;
    Ok(token_data)
}

pub fn save_password(
    secure_store: &SecureStore,
    account_id: &str,
    password: &str,
) -> Result<(), AppError> {
    let key = format!("password_{}", account_id);
    let value = serde_json::json!({ "password": password }).to_string();
    secure_store.insert(&key, value.as_bytes())
}

pub fn load_password(secure_store: &SecureStore, account_id: &str) -> Result<String, AppError> {
    let key = format!("password_{}", account_id);
    let value = secure_store.get(&key)?.ok_or_else(|| {
        AppError::Stronghold(format!("No password found for account {}", account_id))
    })?;

    #[derive(serde::Deserialize)]
    struct PasswordData {
        password: String,
    }
    let data: PasswordData = serde_json::from_slice(&value)
        .map_err(|e| AppError::Stronghold(format!("Failed to deserialize password: {}", e)))?;
    Ok(data.password)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// 分割到着を模したモックリーダー。1回の read で1チャンクだけ返す
    struct ChunkedReader {
        chunks: VecDeque<Vec<u8>>,
    }

    impl ChunkedReader {
        fn new(chunks: &[&[u8]]) -> Self {
            Self {
                chunks: chunks.iter().map(|c| c.to_vec()).collect(),
            }
        }
    }

    impl std::io::Read for ChunkedReader {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            match self.chunks.pop_front() {
                Some(chunk) => {
                    buf[..chunk.len()].copy_from_slice(&chunk);
                    Ok(chunk.len())
                }
                None => Ok(0),
            }
        }
    }

    #[test]
    fn test_read_request_line_single_read() {
        let mut r = ChunkedReader::new(&[
            b"GET /oauth/callback?code=abc&state=xyz HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
        ]);
        let line = read_request_line(&mut r).unwrap();
        assert_eq!(line, "GET /oauth/callback?code=abc&state=xyz HTTP/1.1");
    }

    #[test]
    fn test_read_request_line_split_across_reads() {
        // リクエストラインが複数の read に分割されて届くケース（従来の単一 read 実装
        // ではコールバックを取りこぼしていた）
        let mut r = ChunkedReader::new(&[
            b"GET /oauth/call",
            b"back?code=abc&state=xy",
            b"z HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
        ]);
        let line = read_request_line(&mut r).unwrap();
        assert_eq!(line, "GET /oauth/callback?code=abc&state=xyz HTTP/1.1");
    }

    #[test]
    fn test_read_request_line_eof_without_newline_returns_partial() {
        // 改行前に接続が閉じられた場合は読めた分を返す（従来挙動の維持）
        let mut r = ChunkedReader::new(&[b"GET /oauth/callback?code=a HTTP/1.1"]);
        let line = read_request_line(&mut r).unwrap();
        assert_eq!(line, "GET /oauth/callback?code=a HTTP/1.1");
    }

    #[test]
    fn test_read_request_line_empty_input_returns_none() {
        let mut r = ChunkedReader::new(&[]);
        assert!(read_request_line(&mut r).is_none());
    }

    #[test]
    fn test_read_request_line_gives_up_after_max_bytes() {
        // 改行を含まないデータが上限を超えて届き続けたら打ち切る（メモリ膨張ガード）
        let garbage = vec![b'a'; 1024];
        let chunks: Vec<&[u8]> = (0..9).map(|_| garbage.as_slice()).collect();
        let mut r = ChunkedReader::new(&chunks);
        assert!(read_request_line(&mut r).is_none());
    }

    #[test]
    fn test_parse_request_target_get() {
        let target =
            parse_request_target("GET /oauth/callback?code=abc HTTP/1.1\r\nHost: x\r\n\r\n");
        assert_eq!(target, Some("/oauth/callback?code=abc"));
    }

    #[test]
    fn test_parse_request_target_rejects_non_get() {
        assert!(parse_request_target("POST /oauth/callback HTTP/1.1").is_none());
    }
}
