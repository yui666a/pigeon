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
    let ollama_endpoint =
        settings::get_or_default(conn, "ollama_endpoint", "http://localhost:11434");
    let ollama_model = settings::get_or_default(conn, "ollama_model", "llama3.1:8b");
    let claude_model = settings::get_or_default(conn, "claude_model", "claude-haiku-4-5");
    // 保存済み設定からの構築では、Claude キーは常に SecureStore を参照する。
    build_classifier_from_params(
        &provider,
        &ollama_endpoint,
        &ollama_model,
        &claude_model,
        None,
        secure_store,
    )
}

/// 明示的に渡されたパラメータから Classifier を構築する（保存済み設定に依存しない）。
/// 接続テストのように「まだ保存していない画面上の設定」を検証する用途で使う。
///
/// Claude の API キーは次の優先順で解決する:
/// 1. `claude_api_key` が Some かつ非空ならそれを使う（新規入力の検証）
/// 2. なければ SecureStore の保存済みキーを使う（登録済みキーの再テスト）
/// 3. どちらも無ければ `MissingApiKey`
///
/// フォールバック（別プロバイダへの切替）はしない。
pub fn build_classifier_from_params(
    provider: &str,
    ollama_endpoint: &str,
    ollama_model: &str,
    claude_model: &str,
    claude_api_key: Option<&str>,
    secure_store: &SecureStore,
) -> Result<Box<dyn LlmClassifier>, AppError> {
    match provider {
        "ollama" => Ok(Box::new(OllamaClassifier::new(
            ollama_endpoint,
            ollama_model,
        )?)),
        "claude" => {
            let key = claude_api_key
                .map(|s| s.to_string())
                .filter(|s| !s.trim().is_empty())
                .or_else(|| {
                    secure_store
                        .get(CLAUDE_API_KEY)
                        .ok()
                        .flatten()
                        .and_then(|bytes| String::from_utf8(bytes).ok())
                        .filter(|s| !s.trim().is_empty())
                })
                .ok_or_else(|| AppError::MissingApiKey("claude".to_string()))?;
            Ok(Box::new(ClaudeClassifier::new(key, claude_model)?))
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

    // --- build_classifier_from_params: 明示パラメータからの構築（接続テスト用） ---

    #[test]
    fn test_from_params_ollama_ignores_stored_settings() {
        let (_c, store, _d) = setup();
        // DB に何も保存していなくても、明示パラメータだけで ollama を構築できる
        let result = build_classifier_from_params(
            "ollama",
            "http://localhost:11434",
            "llama3.1:8b",
            "claude-haiku-4-5",
            None,
            &store,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_from_params_claude_uses_explicit_key() {
        let (_c, store, _d) = setup();
        // 保存済みキーが無くても、明示的に渡したキーで構築できる（未保存テストのケース）
        let result = build_classifier_from_params(
            "claude",
            "http://localhost:11434",
            "llama3.1:8b",
            "claude-haiku-4-5",
            Some("sk-ant-explicit"),
            &store,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_from_params_claude_falls_back_to_stored_key() {
        let (_c, store, _d) = setup();
        store.insert("claude_api_key", b"sk-ant-stored").unwrap();
        // 明示キーが None なら保存済みキーを使う（登録済みキーの再テスト）
        let result = build_classifier_from_params(
            "claude",
            "http://localhost:11434",
            "llama3.1:8b",
            "claude-haiku-4-5",
            None,
            &store,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_from_params_claude_empty_explicit_key_falls_back() {
        let (_c, store, _d) = setup();
        store.insert("claude_api_key", b"sk-ant-stored").unwrap();
        // 空文字の明示キーは「未入力」扱いで保存済みキーにフォールバック
        let result = build_classifier_from_params(
            "claude",
            "http://localhost:11434",
            "llama3.1:8b",
            "claude-haiku-4-5",
            Some("   "),
            &store,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_from_params_claude_no_key_anywhere_errs() {
        let (_c, store, _d) = setup();
        // 明示キーも保存済みキーも無ければ MissingApiKey
        let err = match build_classifier_from_params(
            "claude",
            "http://localhost:11434",
            "llama3.1:8b",
            "claude-haiku-4-5",
            None,
            &store,
        ) {
            Err(e) => e,
            Ok(_) => panic!("expected MissingApiKey error"),
        };
        assert!(matches!(err, AppError::MissingApiKey(_)));
    }

    #[test]
    fn test_from_params_openai_errs_unsupported() {
        let (_c, store, _d) = setup();
        let err = match build_classifier_from_params(
            "openai",
            "http://localhost:11434",
            "llama3.1:8b",
            "claude-haiku-4-5",
            None,
            &store,
        ) {
            Err(e) => e,
            Ok(_) => panic!("expected UnsupportedProvider error"),
        };
        assert!(matches!(err, AppError::UnsupportedProvider(_)));
    }
}
