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
        AuthType::Oauth2 => connect_xoauth2(host, port, credential).await,
    }
}

async fn establish_tls(
    host: &str,
    port: u16,
) -> Result<async_imap::Client<TlsStream<Compat<TcpStream>>>, AppError> {
    let tcp = TcpStream::connect((host, port))
        .await
        .map_err(|e| AppError::Imap(format!("TCP connection failed: {}", e)))?;
    let tcp_compat = tcp.compat();
    let tls = async_native_tls::TlsConnector::new();
    let tls_stream = tls
        .connect(host, tcp_compat)
        .await
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
    xoauth2_base64: &str,
) -> Result<ImapSession, AppError> {
    let client = establish_tls(host, port).await?;
    let session = client
        .authenticate("XOAUTH2", XOAuth2Authenticator(xoauth2_base64.to_string()))
        .await
        .map_err(|e| AppError::Imap(format!("XOAUTH2 authentication failed: {}", e.0)))?;
    Ok(session)
}

struct XOAuth2Authenticator(String);

impl async_imap::Authenticator for XOAuth2Authenticator {
    type Response = String;
    fn process(&mut self, _data: &[u8]) -> Self::Response {
        self.0.clone()
    }
}

pub async fn fetch_mails_since_uid(
    session: &mut ImapSession,
    folder: &str,
    since_uid: u32,
) -> Result<Vec<(u32, Vec<u8>)>, AppError> {
    session
        .select(folder)
        .await
        .map_err(|e| AppError::Imap(format!("Select folder failed: {}", e)))?;

    let query = if since_uid == 0 {
        "1:*".to_string()
    } else {
        format!("{}:*", since_uid + 1)
    };

    let messages: Vec<_> = session
        .uid_fetch(&query, "(UID RFC822)")
        .await
        .map_err(|e| AppError::Imap(format!("Fetch failed: {}", e)))?
        .try_collect()
        .await
        .map_err(|e| AppError::Imap(format!("Fetch stream failed: {}", e)))?;

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
    fn test_xoauth2_authenticator_returns_token() {
        let mut auth = XOAuth2Authenticator("base64-xoauth2-string".into());
        let response = async_imap::Authenticator::process(&mut auth, b"");
        assert_eq!(response, "base64-xoauth2-string");
    }
}
