use tauri::State;

use crate::classifier::factory::build_classifier;
use crate::db::settings;
use crate::error::AppError;
use crate::models::settings::LlmSettings;
use crate::secure_store::SecureStore;
use crate::state::{DbState, SecureStoreState};
use rusqlite::Connection;

const CLAUDE_API_KEY: &str = "claude_api_key";

pub(crate) fn load_llm_settings(
    conn: &Connection,
    store: &SecureStore,
) -> Result<LlmSettings, AppError> {
    let claude_api_key_set = store
        .get(CLAUDE_API_KEY)?
        .and_then(|b| String::from_utf8(b).ok())
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    Ok(LlmSettings {
        provider: settings::get_or_default(conn, "llm_provider", "ollama"),
        ollama_endpoint: settings::get_or_default(conn, "ollama_endpoint", "http://localhost:11434"),
        ollama_model: settings::get_or_default(conn, "ollama_model", "llama3.1:8b"),
        claude_model: settings::get_or_default(conn, "claude_model", "claude-haiku-4-5"),
        claude_api_key_set,
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn store_llm_settings(
    conn: &Connection,
    store: &SecureStore,
    provider: &str,
    ollama_endpoint: &str,
    ollama_model: &str,
    claude_model: &str,
    claude_api_key: Option<String>,
) -> Result<(), AppError> {
    settings::set(conn, "llm_provider", provider)?;
    settings::set(conn, "ollama_endpoint", ollama_endpoint)?;
    settings::set(conn, "ollama_model", ollama_model)?;
    settings::set(conn, "claude_model", claude_model)?;
    // 空文字は「変更しない」。既存キーを保持する。
    if let Some(key) = claude_api_key {
        if !key.trim().is_empty() {
            store.insert(CLAUDE_API_KEY, key.as_bytes())?;
        }
    }
    Ok(())
}

#[tauri::command]
pub fn get_llm_settings(
    db: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
) -> Result<LlmSettings, AppError> {
    let conn = db.0.lock().map_err(AppError::lock_err)?;
    load_llm_settings(&conn, &secure_store.0)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn set_llm_settings(
    db: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    provider: String,
    ollama_endpoint: String,
    ollama_model: String,
    claude_model: String,
    claude_api_key: Option<String>,
) -> Result<(), AppError> {
    let conn = db.0.lock().map_err(AppError::lock_err)?;
    store_llm_settings(
        &conn,
        &secure_store.0,
        &provider,
        &ollama_endpoint,
        &ollama_model,
        &claude_model,
        claude_api_key,
    )
}

#[tauri::command]
pub async fn test_llm_connection(
    db: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
) -> Result<(), AppError> {
    let classifier = {
        let conn = db.0.lock().map_err(AppError::lock_err)?;
        build_classifier(&conn, &secure_store.0)?
    };
    classifier.health_check().await
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
        let key = sha2::Sha256::digest(b"pw-123456");
        let store = SecureStore::new(dir.path().join("t.stronghold"), &key).unwrap();
        (conn, store, dir)
    }

    #[test]
    fn test_defaults_when_unset() {
        let (conn, store, _d) = setup();
        let s = load_llm_settings(&conn, &store).unwrap();
        assert_eq!(s.provider, "ollama");
        assert_eq!(s.claude_model, "claude-haiku-4-5");
        assert!(!s.claude_api_key_set);
    }

    #[test]
    fn test_store_then_load_roundtrip() {
        let (conn, store, _d) = setup();
        store_llm_settings(
            &conn, &store, "claude", "http://x:11434", "llama3.1:8b",
            "claude-sonnet-5", Some("sk-ant-xxx".to_string()),
        )
        .unwrap();
        let s = load_llm_settings(&conn, &store).unwrap();
        assert_eq!(s.provider, "claude");
        assert_eq!(s.claude_model, "claude-sonnet-5");
        assert!(s.claude_api_key_set);
    }

    #[test]
    fn test_empty_key_preserves_existing() {
        let (conn, store, _d) = setup();
        store.insert(CLAUDE_API_KEY, b"existing-key").unwrap();
        store_llm_settings(
            &conn, &store, "claude", "http://localhost:11434", "llama3.1:8b",
            "claude-haiku-4-5", Some("".to_string()),
        )
        .unwrap();
        // 空文字入力では既存キーが保持される
        let s = load_llm_settings(&conn, &store).unwrap();
        assert!(s.claude_api_key_set);
    }
}
