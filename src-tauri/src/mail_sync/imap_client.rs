use async_imap::Session;
use async_native_tls::TlsStream;
use futures::TryStreamExt;
use tokio::net::TcpStream;
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt};

use crate::error::AppError;

type ImapSession = Session<TlsStream<Compat<TcpStream>>>;

pub async fn connect(
    host: &str,
    port: u16,
    username: &str,
    password: &str,
) -> Result<ImapSession, AppError> {
    let tcp = TcpStream::connect((host, port))
        .await
        .map_err(|e| AppError::Imap(format!("TCP connection failed: {}", e)))?;
    let tcp_compat = tcp.compat();
    let tls = async_native_tls::TlsConnector::new();
    let tls_stream = tls
        .connect(host, tcp_compat)
        .await
        .map_err(|e| AppError::Imap(format!("TLS handshake failed: {}", e)))?;
    let client = async_imap::Client::new(tls_stream);
    let session = client
        .login(username, password)
        .await
        .map_err(|e| AppError::Imap(format!("Login failed: {}", e.0)))?;
    Ok(session)
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
