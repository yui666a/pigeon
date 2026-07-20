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
            )?;

            oauth_store.cleanup_expired()?;

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
        handle_oauth_callback_inner(&state, secure_store.get()?, &oauth_store, &url).await?;
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
        .take(&state_param)?
        .ok_or(AppError::InvalidOAuthState)?;

    // Check if the pending OAuth entry has expired
    if oauth::is_pending_expired(pending.created_at, oauth::current_timestamp()) {
        return Err(AppError::OAuthTimeout);
    }

    let config = OAuthConfig::google_with_redirect(pending.redirect_uri.clone())?;

    // Exchange authorization code for tokens
    let token_response = oauth::exchange_code(&config, &code, &pending.code_verifier).await?;

    // Extract email from ID token (JWKS で署名検証してから取り出す)
    let email = match &token_response.id_token {
        Some(id_token) => {
            let jwks = oauth::fetch_google_jwks().await?;
            oauth::verify_id_token_email(id_token, &config.client_id, &jwks)?
        }
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
                // Compensating action: remove token from SecureStore if DB insert fails.
                // 分割保管しているため両方のキーを消す（孤立した秘密を残さない）
                let _ = secure_store.delete(&format!("oauth_{}", pending.account_id));
                let _ = secure_store.delete(&access_token_cache_key(&pending.account_id));
                Err(e)
            }
        }
    })
}

/// 再取得不能な部分（リフレッシュトークン・メールアドレス）。
/// `oauth_{id}` に置き、書き込み時に必ずコミットされる。
#[derive(serde::Serialize, serde::Deserialize)]
struct OAuthDurablePart {
    refresh_token: String,
    email: String,
}

/// 再取得可能な部分（アクセストークンと有効期限）。
/// `access_token_cache_{id}` に置き、コミットは遅延させてよい。
#[derive(serde::Serialize, serde::Deserialize)]
struct OAuthCachedPart {
    access_token: String,
    expires_at: i64,
}

fn access_token_cache_key(account_id: &str) -> String {
    format!(
        "{}{}",
        crate::secure_store::ACCESS_TOKEN_CACHE_PREFIX,
        account_id
    )
}

/// OAuth トークンを耐久性クラスごとに分けて保管する。
///
/// アクセストークンは 1 時間ごとに更新されるが、リフレッシュトークンから
/// いつでも再取得できる。同じキーにまとめていると更新のたびに
/// 「再取得不能な秘密の書き込み」となり、毎回スナップショット全体の
/// 再暗号化（scrypt）が走る。揮発する側を分離することで、
/// 定常運転でのコミットを無くす（ADR 0006 決定 4）。
pub fn save_oauth_token(
    secure_store: &SecureStore,
    account_id: &str,
    token_data: &OAuthTokenData,
) -> Result<(), AppError> {
    let durable = OAuthDurablePart {
        refresh_token: token_data.refresh_token.clone(),
        email: token_data.email.clone(),
    };
    let durable_value = serde_json::to_vec(&durable)
        .map_err(|e| AppError::Stronghold(format!("Failed to serialize token data: {}", e)))?;
    // 先に再取得不能な側を確定させる。順序を逆にすると、間で異常終了した場合に
    // アクセストークンだけが残り、リフレッシュトークンを失う
    secure_store.insert(&format!("oauth_{}", account_id), &durable_value)?;

    let cached = OAuthCachedPart {
        access_token: token_data.access_token.clone(),
        expires_at: token_data.expires_at,
    };
    let cached_value = serde_json::to_vec(&cached)
        .map_err(|e| AppError::Stronghold(format!("Failed to serialize token data: {}", e)))?;
    secure_store.insert(&access_token_cache_key(account_id), &cached_value)
}

/// 保管された OAuth トークンを組み立てて返す。
///
/// アクセストークン側は異常終了で失われうる。その場合は空の
/// アクセストークンと期限切れの `expires_at` を返し、呼び出し側の
/// `token_needs_refresh` によって次回同期で再取得させる（再認証は不要）。
pub fn load_oauth_token(
    secure_store: &SecureStore,
    account_id: &str,
) -> Result<OAuthTokenData, AppError> {
    let value = secure_store
        .get(&format!("oauth_{}", account_id))?
        .ok_or_else(|| {
            AppError::Stronghold(format!("No OAuth token found for account {}", account_id))
        })?;
    let durable: OAuthDurablePart = serde_json::from_slice(&value)
        .map_err(|e| AppError::Stronghold(format!("Failed to deserialize token data: {}", e)))?;

    // 遅延コミット分が失われていても、リフレッシュトークンがあれば回復できる。
    // 壊れていた場合も同様に「期限切れ」として扱い、再取得に倒す
    let cached = secure_store
        .get(&access_token_cache_key(account_id))?
        .and_then(|v| serde_json::from_slice::<OAuthCachedPart>(&v).ok());

    let (access_token, expires_at) = match cached {
        Some(c) => (c.access_token, c.expires_at),
        None => (String::new(), 0),
    };

    Ok(OAuthTokenData {
        access_token,
        refresh_token: durable.refresh_token,
        expires_at,
        email: durable.email,
    })
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

    // --- OAuth トークンの保管分割（ADR 0006 決定 4） ---
    //
    // アクセストークンは 1 時間ごとに更新されるが、リフレッシュトークンから
    // いつでも再取得できる。両者を同じキーにまとめていると、更新のたびに
    // 再取得不能な秘密の書き込みとして扱われ、毎回 scrypt が走る。
    // 揮発する側を別キーへ分離し、遅延コミットの対象にする。

    fn token_data(access: &str, refresh: &str) -> OAuthTokenData {
        OAuthTokenData {
            access_token: access.into(),
            refresh_token: refresh.into(),
            expires_at: 1_000,
            email: "user@example.com".into(),
        }
    }

    #[test]
    fn test_save_and_load_oauth_token_roundtrip() {
        // 分割保管しても呼び出し側から見える値は変わらない
        let store = SecureStore::in_memory();
        save_oauth_token(&store, "acc1", &token_data("at", "rt")).unwrap();

        let loaded = load_oauth_token(&store, "acc1").unwrap();
        assert_eq!(loaded.access_token, "at");
        assert_eq!(loaded.refresh_token, "rt");
        assert_eq!(loaded.expires_at, 1_000);
        assert_eq!(loaded.email, "user@example.com");
    }

    #[test]
    fn test_access_token_is_stored_under_deferrable_key() {
        // アクセストークンは遅延コミット対象のキーに置かれる
        let store = SecureStore::in_memory();
        save_oauth_token(&store, "acc1", &token_data("at", "rt")).unwrap();

        let cache_key = format!("{}acc1", crate::secure_store::ACCESS_TOKEN_CACHE_PREFIX);
        assert_eq!(
            crate::secure_store::Durability::of_key(&cache_key),
            crate::secure_store::Durability::Deferrable
        );
        assert!(
            store.get(&cache_key).unwrap().is_some(),
            "アクセストークンは別キーに保管される"
        );
    }

    #[test]
    fn test_refresh_token_blob_does_not_contain_access_token() {
        // oauth_ 側（即コミット）には揮発する値を入れない。
        // ここに入れてしまうと更新のたびに scrypt が走り、分割の意味が無くなる
        let store = SecureStore::in_memory();
        save_oauth_token(&store, "acc1", &token_data("at-secret", "rt")).unwrap();

        let blob = store.get("oauth_acc1").unwrap().unwrap();
        let text = String::from_utf8(blob).unwrap();
        assert!(text.contains("rt"), "リフレッシュトークンは即コミット側");
        assert!(
            !text.contains("at-secret"),
            "アクセストークンは即コミット側に含めない"
        );
    }

    #[test]
    fn test_load_oauth_token_survives_lost_access_token_cache() {
        // 異常終了で遅延分（アクセストークン）が失われた状況の再現。
        // リフレッシュトークンが残っていれば再認証は不要で、
        // 期限切れ扱いにして次回同期で再取得させる
        let store = SecureStore::in_memory();
        save_oauth_token(&store, "acc1", &token_data("at", "rt")).unwrap();
        store
            .delete(&format!(
                "{}acc1",
                crate::secure_store::ACCESS_TOKEN_CACHE_PREFIX
            ))
            .unwrap();

        let loaded = load_oauth_token(&store, "acc1").unwrap();
        assert_eq!(loaded.refresh_token, "rt", "再認証は不要");
        assert!(
            crate::mail_sync::oauth::token_needs_refresh(&loaded),
            "アクセストークンが無い場合は要更新として扱い、次回同期で再取得する"
        );
    }

    #[test]
    fn test_load_oauth_token_reads_legacy_combined_blob() {
        // 分割前（〜2026-07）の既存ユーザーのスナップショット互換性。
        // 旧形式は access_token / expires_at も同じ JSON に入っている。
        // ここで読めないと既存ユーザーが再認証を強いられる
        let store = SecureStore::in_memory();
        let legacy = serde_json::to_vec(&token_data("old-at", "old-rt")).unwrap();
        store.insert("oauth_acc1", &legacy).unwrap();

        let loaded = load_oauth_token(&store, "acc1").unwrap();
        assert_eq!(
            loaded.refresh_token, "old-rt",
            "旧形式からリフレッシュトークンを読める（再認証不要）"
        );
        assert_eq!(loaded.email, "user@example.com");
        assert!(
            crate::mail_sync::oauth::token_needs_refresh(&loaded),
            "アクセストークンは新キーへ移るまで期限切れ扱いとし、次回同期で再取得する"
        );
    }

    #[test]
    fn test_load_oauth_token_errors_when_refresh_token_missing() {
        // リフレッシュトークンが無ければ回復不能（再認証が要る）
        let store = SecureStore::in_memory();
        assert!(load_oauth_token(&store, "acc1").is_err());
    }

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
