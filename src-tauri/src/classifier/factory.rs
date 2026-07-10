use rusqlite::Connection;

use crate::classifier::claude::ClaudeClassifier;
use crate::classifier::ollama::OllamaClassifier;
use crate::classifier::LlmClassifier;
use crate::db::settings;
use crate::error::AppError;
use crate::secure_store::SecureStore;

const CLAUDE_API_KEY: &str = "claude_api_key";

/// 保存済み設定からプロバイダを判定し、対応する Classifier を構築する。
/// フォールバックはしない（設定と実挙動を一致させるため）。
pub fn build_classifier(
    conn: &Connection,
    secure_store: &SecureStore,
) -> Result<Box<dyn LlmClassifier>, AppError> {
    let provider = settings::get_or_default(conn, "llm_provider", "ollama");
    match provider.as_str() {
        "ollama" => {
            let endpoint =
                settings::get_or_default(conn, "ollama_endpoint", "http://localhost:11434");
            let model = settings::get_or_default(conn, "ollama_model", "llama3.1:8b");
            Ok(Box::new(OllamaClassifier::new(endpoint, model)?))
        }
        "claude" => {
            let key = secure_store
                .get(CLAUDE_API_KEY)?
                .and_then(|bytes| String::from_utf8(bytes).ok())
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| AppError::MissingApiKey("claude".to_string()))?;
            let model = settings::get_or_default(conn, "claude_model", "claude-haiku-4-5");
            Ok(Box::new(ClaudeClassifier::new(key, model)?))
        }
        other => Err(AppError::UnsupportedProvider(other.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrations::run_migrations;
    use sha2::Digest;
    use tempfile::TempDir;

    fn setup() -> (Connection, SecureStore, TempDir) {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        let dir = TempDir::new().unwrap();
        // SecureStore/Stronghold expects a fixed-size (32-byte) key, so hash the
        // test password the same way lib.rs derives the real key (see lib.rs).
        let key = sha2::Sha256::digest(b"test-password-123");
        let store = SecureStore::new(dir.path().join("test.stronghold"), &key).unwrap();
        (conn, store, dir)
    }

    #[test]
    fn test_default_provider_is_ollama() {
        let (conn, store, _d) = setup();
        // llm_provider 未設定 → ollama として構築でき、エラーにならない
        assert!(build_classifier(&conn, &store).is_ok());
    }

    #[test]
    fn test_claude_without_key_errs_missing_api_key() {
        let (conn, store, _d) = setup();
        settings::set(&conn, "llm_provider", "claude").unwrap();
        let err = match build_classifier(&conn, &store) {
            Err(e) => e,
            Ok(_) => panic!("expected MissingApiKey error"),
        };
        assert!(matches!(err, AppError::MissingApiKey(_)));
    }

    #[test]
    fn test_claude_with_key_builds() {
        let (conn, store, _d) = setup();
        settings::set(&conn, "llm_provider", "claude").unwrap();
        store.insert("claude_api_key", b"sk-ant-xxx").unwrap();
        assert!(build_classifier(&conn, &store).is_ok());
    }

    #[test]
    fn test_claude_with_empty_key_errs() {
        let (conn, store, _d) = setup();
        settings::set(&conn, "llm_provider", "claude").unwrap();
        store.insert("claude_api_key", b"   ").unwrap();
        let err = match build_classifier(&conn, &store) {
            Err(e) => e,
            Ok(_) => panic!("expected MissingApiKey error"),
        };
        assert!(matches!(err, AppError::MissingApiKey(_)));
    }

    #[test]
    fn test_openai_errs_unsupported() {
        let (conn, store, _d) = setup();
        settings::set(&conn, "llm_provider", "openai").unwrap();
        let err = match build_classifier(&conn, &store) {
            Err(e) => e,
            Ok(_) => panic!("expected UnsupportedProvider error"),
        };
        assert!(matches!(err, AppError::UnsupportedProvider(_)));
    }
}
