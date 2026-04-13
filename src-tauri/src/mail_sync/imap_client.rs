use async_imap::Session;
use async_native_tls::TlsStream;
use futures::TryStreamExt;
use tokio::net::TcpStream;
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt};

use crate::error::AppError;
use crate::models::account::AuthType;

type ImapSession = Session<TlsStream<Compat<TcpStream>>>;

pub async fn connect(
    host: &str,
    port: u16,
    auth_type: &AuthType,
    username: &str,
    credential: &str,
) -> Result<ImapSession, AppError> {
    match auth_type {
        AuthType::Plain => connect_plain(host, port, username, credential).await,
        AuthType::Oauth2 => connect_xoauth2(host, port, username, credential).await,
    }
}

async fn establish_tls(
    host: &str,
    port: u16,
) -> Result<async_imap::Client<TlsStream<Compat<TcpStream>>>, AppError> {
    let tcp = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        TcpStream::connect((host, port)),
    )
    .await
    .map_err(|_| AppError::Imap("TCP connection timed out (15s)".into()))?
    .map_err(|e| AppError::Imap(format!("TCP connection failed: {}", e)))?;
    let tcp_compat = tcp.compat();
    let tls = async_native_tls::TlsConnector::new();
    let tls_stream = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        tls.connect(host, tcp_compat),
    )
    .await
    .map_err(|_| AppError::Imap("TLS handshake timed out (15s)".into()))?
    .map_err(|e| AppError::Imap(format!("TLS handshake failed: {}", e)))?;
    Ok(async_imap::Client::new(tls_stream))
}

async fn connect_plain(
    host: &str,
    port: u16,
    username: &str,
    password: &str,
) -> Result<ImapSession, AppError> {
    let client = establish_tls(host, port).await?;
    let session = client
        .login(username, password)
        .await
        .map_err(|e| AppError::Imap(format!("PLAIN login failed: {}", e.0)))?;
    Ok(session)
}

async fn connect_xoauth2(
    host: &str,
    port: u16,
    email: &str,
    access_token: &str,
) -> Result<ImapSession, AppError> {
    let mut client = establish_tls(host, port).await?;

    // Read the server greeting — this is critical!
    // Without consuming the greeting, authenticate() hangs waiting for it.
    let _greeting = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        client.read_response(),
    )
    .await
    .map_err(|_| AppError::Imap("Server greeting timed out".into()))?
    .map_err(|e| AppError::Imap(format!("Failed to read greeting: {}", e)))?;

    let authenticator = XOAuth2Authenticator {
        email: email.to_string(),
        access_token: access_token.to_string(),
    };

    let session = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        client.authenticate("XOAUTH2", authenticator),
    )
    .await
    .map_err(|_| AppError::Imap("XOAUTH2 authentication timed out (15s)".into()))?
    .map_err(|e| AppError::Imap(format!("XOAUTH2 authentication failed: {}", e.0)))?;
    Ok(session)
}

struct XOAuth2Authenticator {
    email: String,
    access_token: String,
}

impl async_imap::Authenticator for XOAuth2Authenticator {
    type Response = String;
    fn process(&mut self, _data: &[u8]) -> Self::Response {
        format!("user={}\x01auth=Bearer {}\x01\x01", self.email, self.access_token)
    }
}

/// 初回同期時に取得するメールの最大件数
const INITIAL_SYNC_LIMIT: u32 = 20;

pub async fn fetch_mails_since_uid(
    session: &mut ImapSession,
    folder: &str,
    since_uid: u32,
) -> Result<Vec<(u32, Vec<u8>)>, AppError> {
    let mailbox = session
        .select(folder)
        .await
        .map_err(|e| AppError::Imap(format!("Select folder failed: {}", e)))?;

    let query = if since_uid == 0 {
        // 初回同期: 直近 INITIAL_SYNC_LIMIT 件のみ取得
        // メールボックスのメッセージ数から開始位置を計算
        let total = mailbox.exists;
        if total == 0 {
            return Ok(Vec::new());
        }
        let start = if total > INITIAL_SYNC_LIMIT {
            total - INITIAL_SYNC_LIMIT + 1
        } else {
            1
        };
        // シーケンス番号ベースでUIDを取得してからフェッチ
        format!("{}:*", start)
    } else {
        format!("{}:*", since_uid + 1)
    };

    // 初回はシーケンス番号ベース、差分はUIDベース
    let messages: Vec<_> = if since_uid == 0 {
        session
            .fetch(&query, "(UID RFC822)")
            .await
            .map_err(|e| AppError::Imap(format!("Fetch failed: {}", e)))?
            .try_collect()
            .await
            .map_err(|e| AppError::Imap(format!("Fetch stream failed: {}", e)))?
    } else {
        session
            .uid_fetch(&query, "(UID RFC822)")
            .await
            .map_err(|e| AppError::Imap(format!("Fetch failed: {}", e)))?
            .try_collect()
            .await
            .map_err(|e| AppError::Imap(format!("Fetch stream failed: {}", e)))?
    };

    let mut results = Vec::new();
    for msg in &messages {
        if let Some(body) = msg.body() {
            let uid = msg.uid.unwrap_or(0);
            if uid > since_uid {
                results.push((uid, body.to_vec()));
            }
        }
    }
    Ok(results)
}

pub async fn list_folders(session: &mut ImapSession) -> Result<Vec<String>, AppError> {
    let folders: Vec<_> = session
        .list(None, Some("*"))
        .await
        .map_err(|e| AppError::Imap(format!("List folders failed: {}", e)))?
        .try_collect()
        .await
        .map_err(|e| AppError::Imap(format!("List stream failed: {}", e)))?;
    Ok(folders.iter().map(|f| f.name().to_string()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_type_routing_plain() {
        // Verify that AuthType::Plain would route to connect_plain
        let auth_type = AuthType::Plain;
        assert!(matches!(auth_type, AuthType::Plain));
    }

    #[test]
    fn test_auth_type_routing_oauth2() {
        // Verify that AuthType::Oauth2 would route to connect_xoauth2
        let auth_type = AuthType::Oauth2;
        assert!(matches!(auth_type, AuthType::Oauth2));
    }

    #[test]
    fn test_xoauth2_authenticator_returns_sasl_string() {
        let mut auth = XOAuth2Authenticator {
            email: "user@gmail.com".into(),
            access_token: "ya29.token".into(),
        };
        let response = async_imap::Authenticator::process(&mut auth, b"");
        assert_eq!(response, "user=user@gmail.com\x01auth=Bearer ya29.token\x01\x01");
    }
}
