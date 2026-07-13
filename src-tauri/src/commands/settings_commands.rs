use tauri::State;

use crate::classifier::factory::{build_classifier_from_params, ClassifierParams};
use crate::db::settings;
use crate::error::AppError;
use crate::models::settings::LlmSettings;
use crate::secure_store::SecureStore;
use crate::state::{DbState, SecureStoreState};
use rusqlite::Connection;

const CLAUDE_API_KEY: &str = "claude_api_key";
const VERTEX_SA_JSON: &str = "vertex_sa_json";
const DEFAULT_VERTEX_LOCATION: &str = "global";
const DEFAULT_VERTEX_MODEL: &str = "claude-haiku-4-5@20251001";
const DEFAULT_GEMINI_MODEL: &str = "gemini-3.5-flash";

/// SecureStore に非空の値が保存されているかを返す。
fn secret_is_set(store: &SecureStore, key: &str) -> Result<bool, AppError> {
    Ok(store
        .get(key)?
        .and_then(|b| String::from_utf8(b).ok())
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false))
}

pub(crate) fn load_llm_settings(
    conn: &Connection,
    store: &SecureStore,
) -> Result<LlmSettings, AppError> {
    Ok(LlmSettings {
        provider: settings::get_or_default(conn, "llm_provider", "ollama")?,
        ollama_endpoint: settings::get_or_default(conn, "ollama_endpoint", "http://localhost:11434")?,
        ollama_model: settings::get_or_default(conn, "ollama_model", "llama3.1:8b")?,
        claude_model: settings::get_or_default(conn, "claude_model", "claude-haiku-4-5")?,
        claude_api_key_set: secret_is_set(store, CLAUDE_API_KEY)?,
        vertex_project_id: settings::get_or_default(conn, "vertex_project_id", "")?,
        vertex_location: settings::get_or_default(conn, "vertex_location", DEFAULT_VERTEX_LOCATION)?,
        vertex_model: settings::get_or_default(conn, "vertex_model", DEFAULT_VERTEX_MODEL)?,
        vertex_sa_json_set: secret_is_set(store, VERTEX_SA_JSON)?,
        gemini_model: settings::get_or_default(conn, "gemini_model", DEFAULT_GEMINI_MODEL)?,
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
    vertex_project_id: &str,
    vertex_location: &str,
    vertex_model: &str,
    vertex_sa_json: Option<String>,
    gemini_model: &str,
) -> Result<(), AppError> {
    settings::set(conn, "llm_provider", provider)?;
    settings::set(conn, "ollama_endpoint", ollama_endpoint)?;
    settings::set(conn, "ollama_model", ollama_model)?;
    settings::set(conn, "claude_model", claude_model)?;
    settings::set(conn, "vertex_project_id", vertex_project_id)?;
    settings::set(conn, "vertex_location", vertex_location)?;
    settings::set(conn, "vertex_model", vertex_model)?;
    settings::set(conn, "gemini_model", gemini_model)?;
    // 空文字は「変更しない」。既存の秘密情報を保持する。
    store_secret_if_present(store, CLAUDE_API_KEY, claude_api_key)?;
    store_secret_if_present(store, VERTEX_SA_JSON, vertex_sa_json)?;
    Ok(())
}

/// 値が Some かつ非空のときのみ SecureStore に保存する（空は既存維持）。
fn store_secret_if_present(
    store: &SecureStore,
    key: &str,
    value: Option<String>,
) -> Result<(), AppError> {
    if let Some(v) = value {
        if !v.trim().is_empty() {
            store.insert(key, v.as_bytes())?;
        }
    }
    Ok(())
}

#[tauri::command]
pub fn get_llm_settings(
    db: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
) -> Result<LlmSettings, AppError> {
    db.with_conn(|conn| load_llm_settings(conn, &secure_store.0))
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
    vertex_project_id: String,
    vertex_location: String,
    vertex_model: String,
    vertex_sa_json: Option<String>,
    gemini_model: String,
) -> Result<(), AppError> {
    db.with_conn(|conn| {
        store_llm_settings(
            conn,
            &secure_store.0,
            &provider,
            &ollama_endpoint,
            &ollama_model,
            &claude_model,
            claude_api_key,
            &vertex_project_id,
            &vertex_location,
            &vertex_model,
            vertex_sa_json,
            &gemini_model,
        )
    })
}

/// 画面上の（まだ保存していない）設定で接続を検証する。
/// 保存済み設定ではなく、引数で渡された現在の入力値でファクトリを構築する。
/// `claude_api_key` が空/None のときは SecureStore の保存済みキーにフォールバックする
/// （登録済みキーの再テストを可能にするため）。
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn test_llm_connection(
    secure_store: State<'_, SecureStoreState>,
    provider: String,
    ollama_endpoint: String,
    ollama_model: String,
    claude_model: String,
    claude_api_key: Option<String>,
    vertex_project_id: String,
    vertex_location: String,
    vertex_model: String,
    vertex_sa_json: Option<String>,
    gemini_model: String,
) -> Result<(), AppError> {
    let classifier = build_classifier_from_params(
        &ClassifierParams {
            provider: &provider,
            ollama_endpoint: &ollama_endpoint,
            ollama_model: &ollama_model,
            claude_model: &claude_model,
            claude_api_key: claude_api_key.as_deref(),
            vertex_project_id: &vertex_project_id,
            vertex_location: &vertex_location,
            vertex_model: &vertex_model,
            vertex_sa_json: vertex_sa_json.as_deref(),
            gemini_model: &gemini_model,
        },
        &secure_store.0,
    )?;
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
    fn test_defaults_include_vertex() {
        let (conn, store, _d) = setup();
        let s = load_llm_settings(&conn, &store).unwrap();
        assert_eq!(s.vertex_location, "global");
        assert_eq!(s.vertex_model, "claude-haiku-4-5@20251001");
        assert_eq!(s.vertex_project_id, "");
        assert!(!s.vertex_sa_json_set);
        assert_eq!(s.gemini_model, "gemini-3.5-flash");
    }

    #[test]
    fn test_store_then_load_roundtrip() {
        let (conn, store, _d) = setup();
        store_llm_settings(
            &conn, &store, "claude", "http://x:11434", "llama3.1:8b",
            "claude-sonnet-5", Some("sk-ant-xxx".to_string()),
            "", "us-east5", "claude-haiku-4-5@20251001", None, "gemini-3.5-flash",
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
            "", "us-east5", "claude-haiku-4-5@20251001", None, "gemini-3.5-flash",
        )
        .unwrap();
        // 空文字入力では既存キーが保持される
        let s = load_llm_settings(&conn, &store).unwrap();
        assert!(s.claude_api_key_set);
    }

    #[test]
    fn test_vertex_store_then_load_roundtrip() {
        let (conn, store, _d) = setup();
        store_llm_settings(
            &conn, &store, "claude_vertex", "http://localhost:11434", "llama3.1:8b",
            "claude-haiku-4-5", None,
            "my-gcp-project", "us-east5", "claude-sonnet-5",
            Some("{\"type\":\"service_account\"}".to_string()), "gemini-3.5-flash",
        )
        .unwrap();
        let s = load_llm_settings(&conn, &store).unwrap();
        assert_eq!(s.provider, "claude_vertex");
        assert_eq!(s.vertex_project_id, "my-gcp-project");
        assert_eq!(s.vertex_model, "claude-sonnet-5");
        assert!(s.vertex_sa_json_set);
    }

    #[test]
    fn test_empty_sa_json_preserves_existing() {
        let (conn, store, _d) = setup();
        store.insert(VERTEX_SA_JSON, b"existing-sa").unwrap();
        store_llm_settings(
            &conn, &store, "claude_vertex", "http://localhost:11434", "llama3.1:8b",
            "claude-haiku-4-5", None,
            "my-gcp-project", "us-east5", "claude-haiku-4-5@20251001", Some("".to_string()),
            "gemini-3.5-flash",
        )
        .unwrap();
        // 空文字入力では既存 SA JSON が保持される
        let s = load_llm_settings(&conn, &store).unwrap();
        assert!(s.vertex_sa_json_set);
    }
}
