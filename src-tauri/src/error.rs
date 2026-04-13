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
}

impl From<AppError> for String {
    fn from(err: AppError) -> String {
        err.to_string()
    }
}
