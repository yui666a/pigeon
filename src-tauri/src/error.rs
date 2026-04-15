use serde::Serialize;
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

    #[error("Classifier error: {0}")]
    Classifier(String),

    #[error("Ollama connection failed: {0}")]
    OllamaConnection(String),

    #[error("Invalid LLM response: {0}")]
    InvalidLlmResponse(String),

    #[error("Internal lock error: {0}")]
    LockError(String),
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl AppError {
    pub fn lock_err<T>(e: std::sync::PoisonError<T>) -> Self {
        AppError::LockError(e.to_string())
    }
}
