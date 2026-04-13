use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("IMAP error: {0}")]
    Imap(String),

    #[error("MIME parse error: {0}")]
    MimeParse(String),

    #[error("Account not found: {0}")]
    AccountNotFound(String),

    #[error("Mail not found: {0}")]
    MailNotFound(String),

    #[error("OAuth error: {0}")]
    OAuth(String),

    #[error("Token refresh failed: {0}")]
    TokenRefreshFailed(String),

    #[error("Invalid OAuth state")]
    InvalidOAuthState,

    #[error("OAuth timeout: authorization code not received within time limit")]
    OAuthTimeout,

    #[error("Project not found: {0}")]
    ProjectNotFound(String),

    #[error("Duplicate account: {0}")]
    DuplicateAccount(String),

    #[error("Stronghold error: {0}")]
    Stronghold(String),

    #[error("HTTP request error: {0}")]
    HttpRequest(String),
}

impl From<AppError> for String {
    fn from(err: AppError) -> String {
        err.to_string()
    }
}
