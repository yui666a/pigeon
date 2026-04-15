use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AccountProvider {
    Google,
    Other,
}

impl AccountProvider {
    pub fn supports_oauth(&self) -> bool {
        matches!(self, Self::Google)
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            AccountProvider::Google => "google",
            AccountProvider::Other => "other",
        }
    }
}

impl TryFrom<&str> for AccountProvider {
    type Error = String;
    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
            "google" => Ok(AccountProvider::Google),
            "other" => Ok(AccountProvider::Other),
            other => Err(format!("Unknown provider: {}", other)),
        }
    }
}

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
    pub provider: AccountProvider,
    pub created_at: String,
    #[serde(default)]
    pub needs_reauth: bool,
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
    pub provider: AccountProvider,
    pub password: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthTokenData {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
    pub email: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_account_provider_supports_oauth() {
        assert!(AccountProvider::Google.supports_oauth());
        assert!(!AccountProvider::Other.supports_oauth());
    }

    #[test]
    fn test_account_provider_as_str() {
        assert_eq!(AccountProvider::Google.as_str(), "google");
        assert_eq!(AccountProvider::Other.as_str(), "other");
    }

    #[test]
    fn test_account_provider_try_from_valid() {
        assert_eq!(AccountProvider::try_from("google").unwrap(), AccountProvider::Google);
        assert_eq!(AccountProvider::try_from("other").unwrap(), AccountProvider::Other);
    }

    #[test]
    fn test_account_provider_try_from_invalid() {
        assert!(AccountProvider::try_from("yahoo").is_err());
        assert!(AccountProvider::try_from("").is_err());
    }

    #[test]
    fn test_auth_type_as_str() {
        assert_eq!(AuthType::Plain.as_str(), "plain");
        assert_eq!(AuthType::Oauth2.as_str(), "oauth2");
    }

    #[test]
    fn test_auth_type_try_from_valid() {
        assert!(matches!(AuthType::try_from("plain").unwrap(), AuthType::Plain));
        assert!(matches!(AuthType::try_from("oauth2").unwrap(), AuthType::Oauth2));
    }

    #[test]
    fn test_auth_type_try_from_invalid() {
        assert!(AuthType::try_from("basic").is_err());
        assert!(AuthType::try_from("PLAIN").is_err());
    }

    #[test]
    fn test_account_provider_roundtrip() {
        for provider in [AccountProvider::Google, AccountProvider::Other] {
            let s = provider.as_str();
            let back = AccountProvider::try_from(s).unwrap();
            assert_eq!(back, provider);
        }
    }
}
