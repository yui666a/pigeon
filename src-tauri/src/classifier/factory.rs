use rusqlite::Connection;

use crate::classifier::claude::ClaudeClassifier;
use crate::classifier::claude_vertex::ClaudeVertexClassifier;
use crate::classifier::gemini_vertex::GeminiVertexClassifier;
use crate::classifier::ollama::OllamaClassifier;
use crate::classifier::LlmClassifier;
use crate::db::settings;
use crate::error::AppError;
use crate::secure_store::SecureStore;

const CLAUDE_API_KEY: &str = "claude_api_key";
const VERTEX_SA_JSON: &str = "vertex_sa_json";
const DEFAULT_VERTEX_LOCATION: &str = "global";
const DEFAULT_VERTEX_MODEL: &str = "claude-haiku-4-5@20251001";
const DEFAULT_GEMINI_MODEL: &str = "gemini-3.5-flash";

/// Classifier を構築するための画面/設定由来のパラメータ束。
///
/// 引数の増加でシグネチャが破綻するのを避けるため構造体化している。
/// 秘密情報（`claude_api_key` / `vertex_sa_json`）が Some かつ非空ならそれを使い、
/// そうでなければ `SecureStore` の保存済み値にフォールバックする。
#[derive(Debug, Default)]
pub struct ClassifierParams<'a> {
    pub provider: &'a str,
    pub ollama_endpoint: &'a str,
    pub ollama_model: &'a str,
    pub claude_model: &'a str,
    pub claude_api_key: Option<&'a str>,
    pub vertex_project_id: &'a str,
    pub vertex_location: &'a str,
    pub vertex_model: &'a str,
    pub vertex_sa_json: Option<&'a str>,
    /// Gemini on Vertex 用モデル。project_id/location/SA JSON は Claude Vertex と共通。
    pub gemini_model: &'a str,
}

/// クラウド送信になるプロバイダかどうかの唯一の判定点。
///
/// `allow_cloud_context` 等の送信可否ポリシーを適用するか（= cloud フラグ）は
/// 必ずこの関数で判定する。プロバイダ名は `build_classifier_from_params` の
/// match と対で保守すること。未知のプロバイダはクラウド扱い（誤ってローカル
/// 扱いにして未許可コンテキストを送るより、送らない側に倒すフェイルセーフ）。
pub fn is_cloud_provider(provider: &str) -> bool {
    provider != "ollama"
}

/// 保存済み設定（`llm_provider`）からクラウド送信可否を判定する。
/// 分類・rescan・起動時スキャンの全経路がこれを使う。
pub fn is_cloud_provider_configured(conn: &Connection) -> Result<bool, AppError> {
    let provider = settings::get_or_default(conn, "llm_provider", "ollama")?;
    Ok(is_cloud_provider(&provider))
}

/// 保存済み設定からプロバイダを判定し、対応する Classifier を構築する。
/// フォールバックはしない（設定と実挙動を一致させるため）。
pub fn build_classifier(
    conn: &Connection,
    secure_store: &SecureStore,
) -> Result<Box<dyn LlmClassifier>, AppError> {
    let provider = settings::get_or_default(conn, "llm_provider", "ollama")?;
    let ollama_endpoint =
        settings::get_or_default(conn, "ollama_endpoint", "http://localhost:11434")?;
    let ollama_model = settings::get_or_default(conn, "ollama_model", "llama3.1:8b")?;
    let claude_model = settings::get_or_default(conn, "claude_model", "claude-haiku-4-5")?;
    let vertex_project_id = settings::get_or_default(conn, "vertex_project_id", "")?;
    let vertex_location =
        settings::get_or_default(conn, "vertex_location", DEFAULT_VERTEX_LOCATION)?;
    let vertex_model = settings::get_or_default(conn, "vertex_model", DEFAULT_VERTEX_MODEL)?;
    let gemini_model = settings::get_or_default(conn, "gemini_model", DEFAULT_GEMINI_MODEL)?;
    // 保存済み設定からの構築では、秘密情報は常に SecureStore を参照する（None を渡す）。
    build_classifier_from_params(
        &ClassifierParams {
            provider: &provider,
            ollama_endpoint: &ollama_endpoint,
            ollama_model: &ollama_model,
            claude_model: &claude_model,
            claude_api_key: None,
            vertex_project_id: &vertex_project_id,
            vertex_location: &vertex_location,
            vertex_model: &vertex_model,
            vertex_sa_json: None,
            gemini_model: &gemini_model,
        },
        secure_store,
    )
}

/// 明示的に渡されたパラメータから Classifier を構築する（保存済み設定に依存しない）。
/// 接続テストのように「まだ保存していない画面上の設定」を検証する用途で使う。
///
/// 秘密情報（Claude APIキー / Vertex SA JSON）は次の優先順で解決する:
/// 1. パラメータが Some かつ非空ならそれを使う（新規入力の検証）
/// 2. なければ SecureStore の保存済み値を使う（登録済みの再テスト）
/// 3. どちらも無ければ `MissingApiKey`
///
/// フォールバック（別プロバイダへの切替）はしない。
pub fn build_classifier_from_params(
    params: &ClassifierParams,
    secure_store: &SecureStore,
) -> Result<Box<dyn LlmClassifier>, AppError> {
    match params.provider {
        "ollama" => Ok(Box::new(OllamaClassifier::new(
            params.ollama_endpoint,
            params.ollama_model,
        )?)),
        "claude" => {
            let key = resolve_secret(params.claude_api_key, secure_store, CLAUDE_API_KEY)
                .ok_or_else(|| AppError::MissingApiKey("claude".to_string()))?;
            Ok(Box::new(ClaudeClassifier::new(key, params.claude_model)?))
        }
        "claude_vertex" => {
            let sa_json = resolve_secret(params.vertex_sa_json, secure_store, VERTEX_SA_JSON)
                .ok_or_else(|| AppError::MissingApiKey("claude_vertex".to_string()))?;
            if params.vertex_project_id.trim().is_empty() {
                return Err(AppError::MissingApiKey(
                    "claude_vertex (project id 未設定)".to_string(),
                ));
            }
            Ok(Box::new(ClaudeVertexClassifier::new(
                &sa_json,
                params.vertex_project_id,
                params.vertex_location,
                params.vertex_model,
            )?))
        }
        "gemini_vertex" => {
            let sa_json = resolve_secret(params.vertex_sa_json, secure_store, VERTEX_SA_JSON)
                .ok_or_else(|| AppError::MissingApiKey("gemini_vertex".to_string()))?;
            if params.vertex_project_id.trim().is_empty() {
                return Err(AppError::MissingApiKey(
                    "gemini_vertex (project id 未設定)".to_string(),
                ));
            }
            Ok(Box::new(GeminiVertexClassifier::new(
                &sa_json,
                params.vertex_project_id,
                params.vertex_location,
                params.gemini_model,
            )?))
        }
        other => Err(AppError::UnsupportedProvider(other.to_string())),
    }
}

/// 秘密情報を「明示引数（非空）→ SecureStore の保存済み値」の順で解決する。
fn resolve_secret(
    explicit: Option<&str>,
    secure_store: &SecureStore,
    store_key: &str,
) -> Option<String> {
    explicit
        .map(|s| s.to_string())
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            secure_store
                .get(store_key)
                .ok()
                .flatten()
                .and_then(|bytes| String::from_utf8(bytes).ok())
                .filter(|s| !s.trim().is_empty())
        })
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

    // --- is_cloud_provider: クラウド送信可否ポリシーの唯一の判定点 ---

    #[test]
    fn test_is_cloud_provider_ollama_is_local() {
        assert!(!is_cloud_provider("ollama"));
    }

    #[test]
    fn test_is_cloud_provider_cloud_providers_are_cloud() {
        // Anthropic 直 API だけでなく Vertex 系もクラウド送信
        assert!(is_cloud_provider("claude"));
        assert!(is_cloud_provider("claude_vertex"));
        assert!(is_cloud_provider("gemini_vertex"));
    }

    #[test]
    fn test_is_cloud_provider_unknown_is_cloud() {
        // 未知のプロバイダはクラウド扱い（フェイルセーフ: 誤ってローカル扱いに
        // して未許可コンテキストを送るより、送らない側に倒す）
        assert!(is_cloud_provider("openai"));
    }

    #[test]
    fn test_is_cloud_provider_configured_reads_settings() {
        let (conn, _store, _d) = setup();
        // デフォルト（ollama）はローカル
        assert!(!is_cloud_provider_configured(&conn).unwrap());

        for provider in ["claude", "claude_vertex", "gemini_vertex"] {
            settings::set(&conn, "llm_provider", provider).unwrap();
            assert!(
                is_cloud_provider_configured(&conn).unwrap(),
                "{provider} はクラウド判定になること"
            );
        }

        settings::set(&conn, "llm_provider", "ollama").unwrap();
        assert!(!is_cloud_provider_configured(&conn).unwrap());
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

    /// テスト用の使い捨て SA JSON（実在しないダミー鍵。オンライン検証はされない）。
    const TEST_SA_JSON: &str = include_str!("test_sa.json");

    /// provider と秘密情報だけ差し替えたパラメータを組む小さなヘルパ。
    fn params<'a>(
        provider: &'a str,
        claude_api_key: Option<&'a str>,
        vertex_project_id: &'a str,
        vertex_sa_json: Option<&'a str>,
    ) -> ClassifierParams<'a> {
        ClassifierParams {
            provider,
            ollama_endpoint: "http://localhost:11434",
            ollama_model: "llama3.1:8b",
            claude_model: "claude-haiku-4-5",
            claude_api_key,
            vertex_project_id,
            vertex_location: "global",
            vertex_model: "claude-haiku-4-5@20251001",
            vertex_sa_json,
            gemini_model: "gemini-3.5-flash",
        }
    }

    #[test]
    fn test_from_params_ollama_ignores_stored_settings() {
        let (_c, store, _d) = setup();
        // DB に何も保存していなくても、明示パラメータだけで ollama を構築できる
        let result = build_classifier_from_params(&params("ollama", None, "", None), &store);
        assert!(result.is_ok());
    }

    #[test]
    fn test_from_params_claude_uses_explicit_key() {
        let (_c, store, _d) = setup();
        // 保存済みキーが無くても、明示的に渡したキーで構築できる（未保存テストのケース）
        let result = build_classifier_from_params(
            &params("claude", Some("sk-ant-explicit"), "", None),
            &store,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_from_params_claude_falls_back_to_stored_key() {
        let (_c, store, _d) = setup();
        store.insert("claude_api_key", b"sk-ant-stored").unwrap();
        // 明示キーが None なら保存済みキーを使う（登録済みキーの再テスト）
        let result = build_classifier_from_params(&params("claude", None, "", None), &store);
        assert!(result.is_ok());
    }

    #[test]
    fn test_from_params_claude_empty_explicit_key_falls_back() {
        let (_c, store, _d) = setup();
        store.insert("claude_api_key", b"sk-ant-stored").unwrap();
        // 空文字の明示キーは「未入力」扱いで保存済みキーにフォールバック
        let result = build_classifier_from_params(&params("claude", Some("   "), "", None), &store);
        assert!(result.is_ok());
    }

    #[test]
    fn test_from_params_claude_no_key_anywhere_errs() {
        let (_c, store, _d) = setup();
        // 明示キーも保存済みキーも無ければ MissingApiKey
        let err = match build_classifier_from_params(&params("claude", None, "", None), &store) {
            Err(e) => e,
            Ok(_) => panic!("expected MissingApiKey error"),
        };
        assert!(matches!(err, AppError::MissingApiKey(_)));
    }

    #[test]
    fn test_from_params_openai_errs_unsupported() {
        let (_c, store, _d) = setup();
        let err = match build_classifier_from_params(&params("openai", None, "", None), &store) {
            Err(e) => e,
            Ok(_) => panic!("expected UnsupportedProvider error"),
        };
        assert!(matches!(err, AppError::UnsupportedProvider(_)));
    }

    // --- claude_vertex ---

    #[test]
    fn test_from_params_vertex_with_explicit_sa_builds() {
        let (_c, store, _d) = setup();
        // 明示 SA JSON + project_id があれば構築できる
        let result = build_classifier_from_params(
            &params("claude_vertex", None, "test-project", Some(TEST_SA_JSON)),
            &store,
        );
        assert!(
            result.is_ok(),
            "expected Ok, got err: {}",
            result.err().map(|e| e.to_string()).unwrap_or_default()
        );
    }

    #[test]
    fn test_from_params_vertex_falls_back_to_stored_sa() {
        let (_c, store, _d) = setup();
        store
            .insert("vertex_sa_json", TEST_SA_JSON.as_bytes())
            .unwrap();
        // 明示 SA が None なら保存済み SA を使う
        let result = build_classifier_from_params(
            &params("claude_vertex", None, "test-project", None),
            &store,
        );
        assert!(
            result.is_ok(),
            "expected Ok, got err: {}",
            result.err().map(|e| e.to_string()).unwrap_or_default()
        );
    }

    #[test]
    fn test_from_params_vertex_no_sa_errs() {
        let (_c, store, _d) = setup();
        // SA が明示にも保存済みにも無ければ MissingApiKey
        let err = match build_classifier_from_params(
            &params("claude_vertex", None, "test-project", None),
            &store,
        ) {
            Err(e) => e,
            Ok(_) => panic!("expected MissingApiKey error"),
        };
        assert!(matches!(err, AppError::MissingApiKey(_)));
    }

    #[test]
    fn test_from_params_vertex_missing_project_id_errs() {
        let (_c, store, _d) = setup();
        // SA はあっても project_id が空なら MissingApiKey
        let err = match build_classifier_from_params(
            &params("claude_vertex", None, "", Some(TEST_SA_JSON)),
            &store,
        ) {
            Err(e) => e,
            Ok(_) => panic!("expected MissingApiKey error"),
        };
        assert!(matches!(err, AppError::MissingApiKey(_)));
    }

    #[test]
    fn test_build_classifier_vertex_from_stored_settings() {
        let (conn, store, _d) = setup();
        settings::set(&conn, "llm_provider", "claude_vertex").unwrap();
        settings::set(&conn, "vertex_project_id", "test-project").unwrap();
        store
            .insert("vertex_sa_json", TEST_SA_JSON.as_bytes())
            .unwrap();
        assert!(build_classifier(&conn, &store).is_ok());
    }

    // --- gemini_vertex（SA/project/location は claude_vertex と共通、モデルのみ別）---

    #[test]
    fn test_from_params_gemini_with_explicit_sa_builds() {
        let (_c, store, _d) = setup();
        let result = build_classifier_from_params(
            &params("gemini_vertex", None, "test-project", Some(TEST_SA_JSON)),
            &store,
        );
        assert!(
            result.is_ok(),
            "expected Ok, got err: {}",
            result.err().map(|e| e.to_string()).unwrap_or_default()
        );
    }

    #[test]
    fn test_from_params_gemini_no_sa_errs() {
        let (_c, store, _d) = setup();
        let err = match build_classifier_from_params(
            &params("gemini_vertex", None, "test-project", None),
            &store,
        ) {
            Err(e) => e,
            Ok(_) => panic!("expected MissingApiKey error"),
        };
        assert!(matches!(err, AppError::MissingApiKey(_)));
    }

    #[test]
    fn test_from_params_gemini_missing_project_id_errs() {
        let (_c, store, _d) = setup();
        let err = match build_classifier_from_params(
            &params("gemini_vertex", None, "", Some(TEST_SA_JSON)),
            &store,
        ) {
            Err(e) => e,
            Ok(_) => panic!("expected MissingApiKey error"),
        };
        assert!(matches!(err, AppError::MissingApiKey(_)));
    }

    #[test]
    fn test_build_classifier_gemini_from_stored_settings() {
        let (conn, store, _d) = setup();
        settings::set(&conn, "llm_provider", "gemini_vertex").unwrap();
        settings::set(&conn, "vertex_project_id", "test-project").unwrap();
        store
            .insert("vertex_sa_json", TEST_SA_JSON.as_bytes())
            .unwrap();
        assert!(build_classifier(&conn, &store).is_ok());
    }
}
