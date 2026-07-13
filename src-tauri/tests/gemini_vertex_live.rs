//! Gemini on Vertex AI の実機疎通テスト（ネットワーク＋実 SA JSON を使用）。
//!
//! 次の両方が揃ったときだけ実行し、無い環境では自動 skip（CI を壊さない）:
//!   - `secrets/pigeon-vertex-sa.json`（リポジトリ直下、gitignore 済み）
//!   - 環境変数 `PIGEON_VERTEX_PROJECT`（GCP プロジェクト ID。リポジトリに ID を書かない方針のため）
//!
//! 実行例:
//!   PIGEON_VERTEX_PROJECT=your-project cargo test --test gemini_vertex_live -- --nocapture
//!
//! LOCATION / MODEL も環境変数で上書き可（既定は global / gemini-3.5-flash）。

use pigeon_lib::classifier::gemini_vertex::GeminiVertexClassifier;
use pigeon_lib::classifier::{LlmClassifier, TextGenerator};

/// リポジトリ直下の secrets/ にある実 SA JSON を読む。無ければ None。
fn load_sa_json() -> Option<String> {
    // このテストは src-tauri/ を CWD として実行されるため、一つ上がリポジトリ直下。
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|p| p.join("secrets/pigeon-vertex-sa.json"))?;
    std::fs::read_to_string(path).ok()
}

/// 実機テストに必要な設定を環境から集める。どれか欠ければ None（＝skip）。
/// プロジェクト ID はリポジトリにハードコードせず、環境変数から取る。
fn live_config() -> Option<(String, String, String, String)> {
    let sa = load_sa_json()?;
    let project = std::env::var("PIGEON_VERTEX_PROJECT").ok()?;
    let location = std::env::var("PIGEON_VERTEX_LOCATION").unwrap_or_else(|_| "global".to_string());
    let model = std::env::var("PIGEON_VERTEX_GEMINI_MODEL")
        .unwrap_or_else(|_| "gemini-3.5-flash".to_string());
    Some((sa, project, location, model))
}

#[tokio::test]
async fn live_gemini_health_check() {
    let Some((sa, project, location, model)) = live_config() else {
        eprintln!("skip: secrets/ か PIGEON_VERTEX_PROJECT が無いため実機テストをスキップ");
        return;
    };
    let classifier = GeminiVertexClassifier::new(&sa, &project, &location, &model)
        .expect("SA JSON をロードできる");
    classifier
        .health_check()
        .await
        .expect("Gemini on Vertex への疎通・認証・クォータが通ること");
    eprintln!("✅ health_check 成功");
}

#[tokio::test]
async fn live_gemini_generate_text() {
    let Some((sa, project, location, model)) = live_config() else {
        eprintln!("skip: secrets/ か PIGEON_VERTEX_PROJECT が無いため実機テストをスキップ");
        return;
    };
    let classifier = GeminiVertexClassifier::new(&sa, &project, &location, &model)
        .expect("SA JSON をロードできる");
    let out = classifier
        .generate_text("You reply with a single word.", "Reply with exactly: ok")
        .await
        .expect("generate_text が応答を返すこと");
    eprintln!("✅ generate_text 応答: {out:?}");
    assert!(!out.trim().is_empty(), "応答テキストが空でないこと");
}
