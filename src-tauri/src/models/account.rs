use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub name: String,
    pub email: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub auth_type: AuthType,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthType {
    Plain,
    Oauth2,
}

impl AuthType {
    pub fn as_str(&self) -> &'static str {
        match self {
            AuthType::Plain => "plain",
            AuthType::Oauth2 => "oauth2",
        }
    }
}

impl TryFrom<&str> for AuthType {
    type Error = String;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
            "plain" => Ok(AuthType::Plain),
            "oauth2" => Ok(AuthType::Oauth2),
            other => Err(format!("Unknown auth type: {}", other)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAccountRequest {
    pub name: String,
    pub email: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub auth_type: AuthType,
    pub password: String,
}
