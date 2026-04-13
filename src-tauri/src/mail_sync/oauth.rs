use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::Rng;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::AppError;
use crate::models::account::OAuthTokenData;

// Gmail OAuth configuration
pub const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
pub const GOOGLE_SCOPES: &str = "https://mail.google.com/ openid email";
pub const GOOGLE_IMAP_HOST: &str = "imap.gmail.com";
pub const GOOGLE_IMAP_PORT: u16 = 993;
pub const GOOGLE_SMTP_HOST: &str = "smtp.gmail.com";
pub const GOOGLE_SMTP_PORT: u16 = 587;

const PKCE_VERIFIER_LENGTH: usize = 64;
const PKCE_TTL_SECS: u64 = 600; // 10 minutes

#[derive(Debug, Clone)]
pub struct OAuthConfig {
    pub client_id: String,
    pub client_secret: Option<String>,
    pub redirect_uri: String,
}

impl OAuthConfig {
    pub fn google() -> Result<Self, AppError> {
        Self::google_with_redirect("com.haiso666.pigeon://oauth/callback".into())
    }

    pub fn google_with_redirect(redirect_uri: String) -> Result<Self, AppError> {
        let (platform_name, client_id_keys, client_secret_keys, require_secret) =
            if cfg!(target_os = "ios") {
                (
                    "ios",
                    ["PIGEON_GOOGLE_CLIENT_ID_IOS"],
                    ["PIGEON_GOOGLE_CLIENT_SECRET_IOS"],
                    false,
                )
            } else {
                (
                    "desktop",
                    ["PIGEON_GOOGLE_CLIENT_ID_DESKTOP"],
                    ["PIGEON_GOOGLE_CLIENT_SECRET_DESKTOP"],
                    true,
                )
            };

        let client_id = find_first_nonempty_env(&client_id_keys).ok_or_else(|| {
            AppError::OAuth(format!(
                "Google OAuth client id not set for platform {}. Set one of: {}",
                platform_name,
                client_id_keys.join(", ")
            ))
        })?;
        let client_secret = find_first_nonempty_env(&client_secret_keys);

        if require_secret && client_secret.is_none() {
            return Err(AppError::OAuth(format!(
                "Google OAuth client secret not set for platform {}. Set one of: {}",
                platform_name,
                client_secret_keys.join(", ")
            )));
        }

        Ok(Self {
            client_id,
            client_secret,
            redirect_uri,
        })
    }
}

fn find_first_nonempty_env(keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Ok(value) = std::env::var(key) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

#[derive(Debug, Clone)]
pub struct PendingOAuth {
    pub account_id: String,
    pub code_verifier: String,
    pub redirect_uri: String,
    pub created_at: u64,
}

pub struct OAuthStateStore {
    pub pending: Mutex<HashMap<String, PendingOAuth>>,
}

impl OAuthStateStore {
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
        }
    }

    pub fn store(&self, state: String, pending: PendingOAuth) {
        let mut map = self.pending.lock().expect("OAuthStateStore lock poisoned");
        map.insert(state, pending);
    }

    pub fn take(&self, state: &str) -> Option<PendingOAuth> {
        let mut map = self.pending.lock().expect("OAuthStateStore lock poisoned");
        map.remove(state)
    }

    pub fn cleanup_expired(&self) {
        let now = current_timestamp();
        let mut map = self.pending.lock().expect("OAuthStateStore lock poisoned");
        map.retain(|_, v| now - v.created_at < PKCE_TTL_SECS);
    }
}

pub fn generate_code_verifier() -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
    let mut rng = rand::thread_rng();
    (0..PKCE_VERIFIER_LENGTH)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

pub fn generate_code_challenge(verifier: &str) -> String {
    let hash = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(hash)
}

pub fn generate_state() -> String {
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.gen()).collect();
    URL_SAFE_NO_PAD.encode(&bytes)
}

pub fn build_auth_url(config: &OAuthConfig, state: &str, code_challenge: &str) -> String {
    let params = [
        ("client_id", config.client_id.as_str()),
        ("redirect_uri", config.redirect_uri.as_str()),
        ("response_type", "code"),
        ("scope", GOOGLE_SCOPES),
        ("state", state),
        ("code_challenge", code_challenge),
        ("code_challenge_method", "S256"),
        ("access_type", "offline"),
        ("prompt", "consent"),
    ];

    let query = params
        .iter()
        .map(|(k, v)| format!("{}={}", k, urlencoding(v)))
        .collect::<Vec<_>>()
        .join("&");

    format!("{}?{}", GOOGLE_AUTH_URL, query)
}

fn urlencoding(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                String::from(b as char)
            }
            _ => format!("%{:02X}", b),
        })
        .collect()
}

#[derive(Debug, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: i64,
    pub id_token: Option<String>,
    pub token_type: String,
}

#[derive(Debug, Deserialize)]
struct IdTokenClaims {
    email: Option<String>,
}

pub async fn exchange_code(
    config: &OAuthConfig,
    code: &str,
    code_verifier: &str,
) -> Result<TokenResponse, AppError> {
    let client = reqwest::Client::new();
    let mut params = vec![
        ("code", code),
        ("client_id", config.client_id.as_str()),
        ("redirect_uri", config.redirect_uri.as_str()),
        ("grant_type", "authorization_code"),
        ("code_verifier", code_verifier),
    ];
    if let Some(secret) = config.client_secret.as_deref() {
        params.push(("client_secret", secret));
    }

    let response = client
        .post(GOOGLE_TOKEN_URL)
        .form(&params)
        .send()
        .await
        .map_err(|e| AppError::HttpRequest(format!("Token exchange request failed: {}", e)))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AppError::OAuth(format!(
            "Token exchange failed ({}): {}",
            status, body
        )));
    }

    response
        .json::<TokenResponse>()
        .await
        .map_err(|e| AppError::OAuth(format!("Failed to parse token response: {}", e)))
}

pub async fn refresh_token(
    config: &OAuthConfig,
    refresh_token_value: &str,
) -> Result<TokenResponse, AppError> {
    let client = reqwest::Client::new();
    let mut params = vec![
        ("client_id", config.client_id.as_str()),
        ("refresh_token", refresh_token_value),
        ("grant_type", "refresh_token"),
    ];
    if let Some(secret) = config.client_secret.as_deref() {
        params.push(("client_secret", secret));
    }

    let response = client
        .post(GOOGLE_TOKEN_URL)
        .form(&params)
        .send()
        .await
        .map_err(|e| AppError::HttpRequest(format!("Token refresh request failed: {}", e)))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AppError::TokenRefreshFailed(format!(
            "Token refresh failed ({}): {}",
            status, body
        )));
    }

    response
        .json::<TokenResponse>()
        .await
        .map_err(|e| AppError::OAuth(format!("Failed to parse refresh response: {}", e)))
}

pub fn decode_id_token_email(id_token: &str) -> Result<String, AppError> {
    // JWT format: header.payload.signature
    // We only need the payload, no signature verification needed (direct HTTPS communication)
    let parts: Vec<&str> = id_token.split('.').collect();
    if parts.len() != 3 {
        return Err(AppError::OAuth("Invalid ID token format".into()));
    }

    let payload_bytes = URL_SAFE_NO_PAD
        .decode(parts[1])
        .or_else(|_| {
            // Try with standard base64 padding
            let padded = match parts[1].len() % 4 {
                2 => format!("{}==", parts[1]),
                3 => format!("{}=", parts[1]),
                _ => parts[1].to_string(),
            };
            URL_SAFE_NO_PAD.decode(&padded)
        })
        .map_err(|e| AppError::OAuth(format!("Failed to decode ID token payload: {}", e)))?;

    let claims: IdTokenClaims = serde_json::from_slice(&payload_bytes)
        .map_err(|e| AppError::OAuth(format!("Failed to parse ID token claims: {}", e)))?;

    claims
        .email
        .ok_or_else(|| AppError::OAuth("No email claim in ID token".into()))
}

pub fn build_xoauth2_auth_string(email: &str, access_token: &str) -> String {
    let auth_string = format!("user={}\x01auth=Bearer {}\x01\x01", email, access_token);
    base64::engine::general_purpose::STANDARD.encode(auth_string.as_bytes())
}

pub fn token_needs_refresh(token_data: &OAuthTokenData) -> bool {
    let now = current_timestamp() as i64;
    let buffer_secs = 300; // 5 minutes
    token_data.expires_at - now < buffer_secs
}

pub fn build_token_data(
    token_response: &TokenResponse,
    email: &str,
    existing_refresh_token: Option<&str>,
) -> Result<OAuthTokenData, AppError> {
    let now = current_timestamp() as i64;
    let refresh = token_response
        .refresh_token
        .as_deref()
        .or(existing_refresh_token)
        .ok_or_else(|| AppError::OAuth("No refresh token available".into()))?
        .to_string();

    Ok(OAuthTokenData {
        access_token: token_response.access_token.clone(),
        refresh_token: refresh,
        expires_at: now + token_response.expires_in,
        email: email.to_string(),
    })
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs()
}

pub fn parse_callback_url(url: &str) -> Result<(String, String), AppError> {
    // Parse callback URL query (supports custom-scheme and loopback URLs).
    let query_start = url
        .find('?')
        .ok_or_else(|| AppError::OAuth("No query parameters in callback URL".into()))?;
    let query = &url[query_start + 1..];

    let mut code = None;
    let mut state = None;

    for pair in query.split('&') {
        let mut kv = pair.splitn(2, '=');
        let key = kv.next();
        let value = kv.next();
        match (key, value) {
            (Some("code"), Some(v)) => code = Some(percent_decode(v)?),
            (Some("state"), Some(v)) => state = Some(percent_decode(v)?),
            (Some("error"), Some(v)) => {
                return Err(AppError::OAuth(format!("OAuth error from provider: {}", v)));
            }
            _ => {}
        }
    }

    let code = code.ok_or_else(|| AppError::OAuth("No code in callback URL".into()))?;
    let state = state.ok_or_else(|| AppError::OAuth("No state in callback URL".into()))?;
    Ok((code, state))
}

fn percent_decode(input: &str) -> Result<String, AppError> {
    let bytes = input.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let high = hex_to_u8(bytes[i + 1])?;
                let low = hex_to_u8(bytes[i + 2])?;
                output.push((high << 4) | low);
                i += 3;
            }
            b'+' => {
                output.push(b' ');
                i += 1;
            }
            b => {
                output.push(b);
                i += 1;
            }
        }
    }

    String::from_utf8(output)
        .map_err(|e| AppError::OAuth(format!("Invalid UTF-8 in callback parameter: {}", e)))
}

fn hex_to_u8(c: u8) -> Result<u8, AppError> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        _ => Err(AppError::OAuth(
            "Invalid percent-encoding in callback URL".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> OAuthConfig {
        OAuthConfig {
            client_id: "test-client-id.apps.googleusercontent.com".into(),
            client_secret: Some("test-client-secret".into()),
            redirect_uri: "com.haiso666.pigeon://oauth/callback".into(),
        }
    }

    #[test]
    fn test_generate_code_verifier_length() {
        let verifier = generate_code_verifier();
        assert_eq!(verifier.len(), PKCE_VERIFIER_LENGTH);
    }

    #[test]
    fn test_generate_code_verifier_valid_chars() {
        let verifier = generate_code_verifier();
        for c in verifier.chars() {
            assert!(
                c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == '_' || c == '~',
                "Invalid character in code_verifier: {}",
                c
            );
        }
    }

    #[test]
    fn test_generate_code_verifier_is_random() {
        let v1 = generate_code_verifier();
        let v2 = generate_code_verifier();
        assert_ne!(v1, v2);
    }

    #[test]
    fn test_generate_code_challenge() {
        // Known test vector: SHA-256 of "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"
        // should produce "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = generate_code_challenge(verifier);
        assert_eq!(challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn test_generate_state_is_random() {
        let s1 = generate_state();
        let s2 = generate_state();
        assert_ne!(s1, s2);
    }

    #[test]
    fn test_generate_state_is_base64url() {
        let state = generate_state();
        // Should only contain base64url characters
        for c in state.chars() {
            assert!(
                c.is_ascii_alphanumeric() || c == '-' || c == '_',
                "Invalid character in state: {}",
                c
            );
        }
    }

    #[test]
    fn test_build_auth_url_contains_required_params() {
        let config = test_config();
        let state = "test-state-123";
        let challenge = "test-challenge-456";
        let url = build_auth_url(&config, state, challenge);

        assert!(url.starts_with(GOOGLE_AUTH_URL));
        assert!(url.contains("client_id=test-client-id.apps.googleusercontent.com"));
        assert!(url.contains("redirect_uri=com.haiso666.pigeon%3A%2F%2Foauth%2Fcallback"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("state=test-state-123"));
        assert!(url.contains("code_challenge=test-challenge-456"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("access_type=offline"));
        assert!(url.contains("prompt=consent"));
    }

    #[test]
    fn test_build_auth_url_contains_scopes() {
        let config = test_config();
        let url = build_auth_url(&config, "state", "challenge");
        assert!(url.contains("scope=https%3A%2F%2Fmail.google.com%2F"));
        assert!(url.contains("openid"));
        assert!(url.contains("email"));
    }

    #[test]
    fn test_build_xoauth2_auth_string() {
        let auth = build_xoauth2_auth_string("user@gmail.com", "ya29.access-token");
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&auth)
            .unwrap();
        let decoded_str = String::from_utf8(decoded).unwrap();
        assert_eq!(
            decoded_str,
            "user=user@gmail.com\x01auth=Bearer ya29.access-token\x01\x01"
        );
    }

    #[test]
    fn test_decode_id_token_email() {
        // Construct a fake JWT with email claim
        let header = URL_SAFE_NO_PAD.encode(b"{\"alg\":\"RS256\",\"typ\":\"JWT\"}");
        let payload = URL_SAFE_NO_PAD.encode(b"{\"email\":\"user@gmail.com\",\"sub\":\"12345\"}");
        let signature = URL_SAFE_NO_PAD.encode(b"fake-signature");
        let id_token = format!("{}.{}.{}", header, payload, signature);

        let email = decode_id_token_email(&id_token).unwrap();
        assert_eq!(email, "user@gmail.com");
    }

    #[test]
    fn test_decode_id_token_no_email() {
        let header = URL_SAFE_NO_PAD.encode(b"{\"alg\":\"RS256\"}");
        let payload = URL_SAFE_NO_PAD.encode(b"{\"sub\":\"12345\"}");
        let signature = URL_SAFE_NO_PAD.encode(b"sig");
        let id_token = format!("{}.{}.{}", header, payload, signature);

        let result = decode_id_token_email(&id_token);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_id_token_invalid_format() {
        let result = decode_id_token_email("not-a-jwt");
        assert!(result.is_err());
    }

    #[test]
    fn test_token_needs_refresh() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // Token expiring in 2 minutes — needs refresh (< 5 min buffer)
        let token = OAuthTokenData {
            access_token: "test".into(),
            refresh_token: "test".into(),
            expires_at: now + 120,
            email: "user@gmail.com".into(),
        };
        assert!(token_needs_refresh(&token));

        // Token expiring in 10 minutes — no refresh needed
        let token = OAuthTokenData {
            access_token: "test".into(),
            refresh_token: "test".into(),
            expires_at: now + 600,
            email: "user@gmail.com".into(),
        };
        assert!(!token_needs_refresh(&token));
    }

    #[test]
    fn test_build_token_data() {
        let response = TokenResponse {
            access_token: "ya29.xxx".into(),
            refresh_token: Some("1//xxx".into()),
            expires_in: 3600,
            id_token: None,
            token_type: "Bearer".into(),
        };

        let data = build_token_data(&response, "user@gmail.com", None).unwrap();
        assert_eq!(data.access_token, "ya29.xxx");
        assert_eq!(data.refresh_token, "1//xxx");
        assert_eq!(data.email, "user@gmail.com");
    }

    #[test]
    fn test_build_token_data_uses_existing_refresh_token() {
        let response = TokenResponse {
            access_token: "ya29.new".into(),
            refresh_token: None,
            expires_in: 3600,
            id_token: None,
            token_type: "Bearer".into(),
        };

        let data = build_token_data(&response, "user@gmail.com", Some("1//existing")).unwrap();
        assert_eq!(data.refresh_token, "1//existing");
    }

    #[test]
    fn test_build_token_data_no_refresh_token_fails() {
        let response = TokenResponse {
            access_token: "ya29.xxx".into(),
            refresh_token: None,
            expires_in: 3600,
            id_token: None,
            token_type: "Bearer".into(),
        };

        let result = build_token_data(&response, "user@gmail.com", None);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_callback_url_success() {
        let url = "com.haiso666.pigeon://oauth/callback?code=4/0abc123&state=xyz789";
        let (code, state) = parse_callback_url(url).unwrap();
        assert_eq!(code, "4/0abc123");
        assert_eq!(state, "xyz789");
    }

    #[test]
    fn test_parse_callback_url_success_percent_encoded() {
        let url = "http://127.0.0.1:34567/oauth/callback?code=4%2F0abc123&state=xyz%2B789";
        let (code, state) = parse_callback_url(url).unwrap();
        assert_eq!(code, "4/0abc123");
        assert_eq!(state, "xyz+789");
    }

    #[test]
    fn test_parse_callback_url_error_response() {
        let url = "com.haiso666.pigeon://oauth/callback?error=access_denied&state=xyz";
        let result = parse_callback_url(url);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_callback_url_no_query() {
        let result = parse_callback_url("com.haiso666.pigeon://oauth/callback");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_callback_url_missing_code() {
        let url = "com.haiso666.pigeon://oauth/callback?state=xyz";
        let result = parse_callback_url(url);
        assert!(result.is_err());
    }

    #[test]
    fn test_oauth_state_store() {
        let store = OAuthStateStore::new();
        let pending = PendingOAuth {
            account_id: "acc-123".into(),
            code_verifier: "verifier".into(),
            redirect_uri: "http://127.0.0.1:34567/oauth/callback".into(),
            created_at: current_timestamp(),
        };

        store.store("state-1".into(), pending);

        // Take removes it
        let taken = store.take("state-1");
        assert!(taken.is_some());
        assert_eq!(taken.unwrap().account_id, "acc-123");

        // Second take returns None
        let taken_again = store.take("state-1");
        assert!(taken_again.is_none());
    }

    #[test]
    fn test_oauth_state_store_cleanup_expired() {
        let store = OAuthStateStore::new();

        // Insert an expired entry (created 11 minutes ago)
        let expired = PendingOAuth {
            account_id: "expired".into(),
            code_verifier: "v1".into(),
            redirect_uri: "http://127.0.0.1:34567/oauth/callback".into(),
            created_at: current_timestamp() - PKCE_TTL_SECS - 60,
        };
        store.store("old-state".into(), expired);

        // Insert a fresh entry
        let fresh = PendingOAuth {
            account_id: "fresh".into(),
            code_verifier: "v2".into(),
            redirect_uri: "http://127.0.0.1:34568/oauth/callback".into(),
            created_at: current_timestamp(),
        };
        store.store("new-state".into(), fresh);

        store.cleanup_expired();

        assert!(store.take("old-state").is_none());
        assert!(store.take("new-state").is_some());
    }
}
