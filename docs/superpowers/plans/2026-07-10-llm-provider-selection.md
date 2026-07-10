# LLMプロバイダ選択設定 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 設定ダイアログでLLMプロバイダ（Ollama / Claude / ChatGPT）を選択できるようにし、分類・ダイジェスト生成がその選択に従うようにする。

**Architecture:** 分散していた `OllamaClassifier` の直接構築を `classifier::factory::build_classifier` に集約する。`TextGenerator` を `LlmClassifier` のスーパートレイトに統合し、ファクトリは `Box<dyn LlmClassifier>` を返す。プロバイダ設定は `settings` テーブル、APIキーは `SecureStore`(Stronghold) に保存する。JSON抽出/パースは Ollama から共通モジュールへ引き上げ Claude と共有する。

**Tech Stack:** Rust / Tauri 2 / rusqlite / reqwest(0.12, json+rustls-tls) / async-trait / iota_stronghold、React 19 + TypeScript / Vitest + React Testing Library / Tailwind v4。

## Global Constraints

- `unwrap()` / `expect()` はテストコード以外で使用しない。
- Tauri commands は `Result<T, AppError>`（`AppError` は `Serialize` 実装済みで文字列化される）を返す。
- APIキー等の秘密情報は `settings`（平文SQLite）に保存しない。`SecureStore`(Stronghold) を使う。
- クラウドLLMへ送るデータは「件名・送信者・本文冒頭300文字＋案件リスト＋許可された案件コンテキスト」に限定（既存 `classifier::prompt` を流用）。
- クラウドプロバイダ選択時はUIに警告を表示する。
- Claudeのデフォルトモデルは `claude-haiku-4-5`。Claude API: `POST https://api.anthropic.com/v1/messages`、ヘッダ `content-type: application/json` / `x-api-key: <key>` / `anthropic-version: 2023-06-01`。
- TypeScript で `any` 禁止。invoke レスポンスに型を付ける。
- コミットは意味のある粒度（1コミット=1意図）で Conventional Commits 形式。

## File Structure

**Rust**
- `src-tauri/src/classifier/parse.rs`（新規）— JSON抽出/`ClassifyResult` パース（Ollamaから移動）
- `src-tauri/src/classifier/mod.rs`（変更）— trait統合、`parse`/`factory`/`claude` を pub mod 追加
- `src-tauri/src/classifier/ollama.rs`（変更）— `parse` 利用に置換、`extract_json`/`parse_response` 削除
- `src-tauri/src/classifier/claude.rs`（新規）— `ClaudeClassifier`
- `src-tauri/src/classifier/factory.rs`（新規）— `build_classifier`
- `src-tauri/src/db/settings.rs`（変更）— `set` 追加
- `src-tauri/src/error.rs`（変更）— `MissingApiKey`/`UnsupportedProvider` 追加
- `src-tauri/src/models/settings.rs`（新規）— `LlmSettings`
- `src-tauri/src/models/mod.rs`（変更）— `pub mod settings;`
- `src-tauri/src/commands/settings_commands.rs`（新規）— 3コマンド
- `src-tauri/src/commands/mod.rs`（変更）— `pub mod settings_commands;`
- `src-tauri/src/commands/classify_commands.rs`（変更）— ファクトリ利用へ
- `src-tauri/src/commands/directory_commands.rs`（変更）— ファクトリ利用へ
- `src-tauri/src/lib.rs`（変更）— 起動時スキャン、command 登録

**React**
- `src/types/settings.ts`（新規）
- `src/components/sidebar/LlmSettingsDialog.tsx`（新規）
- `src/__tests__/LlmSettingsDialog.test.tsx`（新規）
- サイドバーに設定ボタン追加（該当コンポーネントは実装時に特定）

---

## Task 1: settings テーブルへの書き込みヘルパー

**Files:**
- Modify: `src-tauri/src/db/settings.rs`
- Test: 同ファイル内 `#[cfg(test)] mod tests`

**Interfaces:**
- Consumes: 既存 `get_or_default(conn, key, default) -> String`
- Produces: `pub fn set(conn: &Connection, key: &str, value: &str) -> Result<(), crate::error::AppError>`

- [ ] **Step 1: 失敗するテストを書く**

`src-tauri/src/db/settings.rs` の `mod tests` に追加:

```rust
    #[test]
    fn test_set_inserts_new_key() {
        let conn = setup_db();
        set(&conn, "llm_provider", "claude").unwrap();
        assert_eq!(get_or_default(&conn, "llm_provider", "ollama"), "claude");
    }

    #[test]
    fn test_set_overwrites_existing_key() {
        let conn = setup_db();
        set(&conn, "llm_provider", "ollama").unwrap();
        set(&conn, "llm_provider", "claude").unwrap();
        assert_eq!(get_or_default(&conn, "llm_provider", "ollama"), "claude");
    }
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `cd src-tauri && cargo test settings::tests::test_set`
Expected: FAIL（`set` 未定義でコンパイルエラー）

- [ ] **Step 3: 最小実装**

`src-tauri/src/db/settings.rs` 冒頭の import を `use crate::error::AppError;` を足しつつ、`get_or_default` の下に追加:

```rust
/// `key` に `value` を UPSERT する。
pub fn set(conn: &Connection, key: &str, value: &str) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}
```

ファイル先頭に `use crate::error::AppError;` を追加（既存の `use rusqlite::{params, Connection};` の下）。

- [ ] **Step 4: テストが通ることを確認**

Run: `cd src-tauri && cargo test settings::tests`
Expected: PASS（既存テスト含め全緑）

- [ ] **Step 5: コミット**

```bash
git add src-tauri/src/db/settings.rs
git commit -m "feat(db): settings テーブルへの UPSERT ヘルパー set を追加"
```

---

## Task 2: AppError にプロバイダ関連エラーを追加

**Files:**
- Modify: `src-tauri/src/error.rs:63`（`LockError` の前）

**Interfaces:**
- Produces: `AppError::MissingApiKey(String)`, `AppError::UnsupportedProvider(String)`

- [ ] **Step 1: バリアント追加**

`src-tauri/src/error.rs` の `InvalidLlmResponse` バリアントの直後（`LockError` の前）に追加:

```rust
    #[error("API key not configured for provider: {0}")]
    MissingApiKey(String),

    #[error("Unsupported LLM provider: {0}")]
    UnsupportedProvider(String),
```

- [ ] **Step 2: コンパイル確認**

Run: `cd src-tauri && cargo build`
Expected: 成功（警告 unused は許容、後続タスクで使用）

- [ ] **Step 3: コミット**

```bash
git add src-tauri/src/error.rs
git commit -m "feat(classifier): MissingApiKey/UnsupportedProvider エラーを追加"
```

---

## Task 3: JSON抽出/パースを共通モジュールへ引き上げ

現在 `OllamaClassifier::extract_json` / `parse_response` にある抽出ロジックを `classifier::parse` へ移し、Ollama/Claude で共有する。

**Files:**
- Create: `src-tauri/src/classifier/parse.rs`
- Modify: `src-tauri/src/classifier/mod.rs`（`pub mod parse;` 追加）
- Modify: `src-tauri/src/classifier/ollama.rs`（`extract_json`/`parse_response` を削除し `parse::parse_classify_result` を使用、関連テストを parse.rs へ移設）
- Test: `src-tauri/src/classifier/parse.rs` 内 `#[cfg(test)]`

**Interfaces:**
- Produces:
  - `pub fn extract_json(content: &str) -> Option<&str>`
  - `pub fn parse_classify_result(content: &str) -> Result<crate::models::classifier::ClassifyResult, crate::error::AppError>`

- [ ] **Step 1: parse.rs を作成（テスト付き）**

`src-tauri/src/classifier/parse.rs`:

```rust
use crate::error::AppError;
use crate::models::classifier::ClassifyResult;

/// 本文中の最初の '{' から最後の '}' までを取り出す。
pub fn extract_json(content: &str) -> Option<&str> {
    let start = content.find('{')?;
    let end = content.rfind('}')?;
    if start <= end {
        Some(&content[start..=end])
    } else {
        None
    }
}

/// LLM 応答テキストから ClassifyResult をパースする。
pub fn parse_classify_result(content: &str) -> Result<ClassifyResult, AppError> {
    let json_str = extract_json(content).ok_or_else(|| {
        AppError::InvalidLlmResponse(format!("No JSON object found in response: {}", content))
    })?;
    serde_json::from_str::<ClassifyResult>(json_str).map_err(|e| {
        AppError::InvalidLlmResponse(format!(
            "Failed to parse ClassifyResult from '{}': {}",
            json_str, e
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::classifier::ClassifyAction;

    #[test]
    fn test_extract_json() {
        let input =
            r#"{"action": "assign", "project_id": "p1", "confidence": 0.9, "reason": "test"}"#;
        assert_eq!(extract_json(input).unwrap(), input);
    }

    #[test]
    fn test_extract_json_with_surrounding_text() {
        let input = r#"Sure: {"action": "unclassified", "confidence": 0.2, "reason": "x"} done"#;
        let out = extract_json(input).unwrap();
        assert!(out.starts_with('{') && out.ends_with('}'));
    }

    #[test]
    fn test_extract_json_no_json() {
        assert!(extract_json("no json here").is_none());
    }

    #[test]
    fn test_extract_json_empty_string() {
        assert!(extract_json("").is_none());
    }

    #[test]
    fn test_extract_json_only_open_brace() {
        assert!(extract_json("{").is_none());
    }

    #[test]
    fn test_extract_json_only_close_brace() {
        assert!(extract_json("}").is_none());
    }

    #[test]
    fn test_extract_json_nested_braces() {
        let input = r#"{"outer": {"inner": "value"}}"#;
        assert_eq!(extract_json(input).unwrap(), input);
    }

    #[test]
    fn test_parse_assign() {
        let content = r#"{"action": "assign", "project_id": "proj-123", "confidence": 0.85, "reason": "r"}"#;
        let result = parse_classify_result(content).unwrap();
        assert!(matches!(result.action, ClassifyAction::Assign { .. }));
        if let ClassifyAction::Assign { project_id } = result.action {
            assert_eq!(project_id, "proj-123");
        }
        assert!((result.confidence - 0.85).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parse_create() {
        let content = r#"{"action": "create", "project_name": "新規", "description": "d", "confidence": 0.75, "reason": "r"}"#;
        let result = parse_classify_result(content).unwrap();
        assert!(matches!(result.action, ClassifyAction::Create { .. }));
    }

    #[test]
    fn test_parse_unclassified() {
        let content = r#"{"action": "unclassified", "confidence": 0.2, "reason": "曖昧"}"#;
        let result = parse_classify_result(content).unwrap();
        assert!(matches!(result.action, ClassifyAction::Unclassified));
    }

    #[test]
    fn test_parse_with_surrounding_text() {
        let content = "結果:\n{\"action\": \"assign\", \"project_id\": \"p\", \"confidence\": 0.9, \"reason\": \"r\"}\nおわり";
        let result = parse_classify_result(content).unwrap();
        assert!(matches!(result.action, ClassifyAction::Assign { .. }));
    }

    #[test]
    fn test_parse_invalid() {
        assert!(parse_classify_result("plain text").is_err());
    }

    #[test]
    fn test_parse_missing_confidence() {
        assert!(parse_classify_result(r#"{"action": "unclassified", "reason": "t"}"#).is_err());
    }

    #[test]
    fn test_parse_missing_reason() {
        assert!(parse_classify_result(r#"{"action": "unclassified", "confidence": 0.5}"#).is_err());
    }

    #[test]
    fn test_parse_unknown_action() {
        assert!(parse_classify_result(r#"{"action": "delete", "confidence": 0.5, "reason": "t"}"#).is_err());
    }

    #[test]
    fn test_parse_assign_missing_project_id() {
        assert!(parse_classify_result(r#"{"action": "assign", "confidence": 0.9, "reason": "t"}"#).is_err());
    }

    #[test]
    fn test_parse_create_missing_fields() {
        assert!(parse_classify_result(r#"{"action": "create", "confidence": 0.7, "reason": "t"}"#).is_err());
    }

    #[test]
    fn test_parse_confidence_boundaries() {
        let r0 = parse_classify_result(r#"{"action": "unclassified", "confidence": 0.0, "reason": "t"}"#).unwrap();
        assert!((r0.confidence - 0.0).abs() < f64::EPSILON);
        let r1 = parse_classify_result(r#"{"action": "unclassified", "confidence": 1.0, "reason": "t"}"#).unwrap();
        assert!((r1.confidence - 1.0).abs() < f64::EPSILON);
    }
}
```

- [ ] **Step 2: mod.rs に登録**

`src-tauri/src/classifier/mod.rs` の先頭を:

```rust
pub mod ollama;
pub mod parse;
pub mod prompt;
```

- [ ] **Step 3: テストが通ることを確認**

Run: `cd src-tauri && cargo test classifier::parse`
Expected: PASS

- [ ] **Step 4: ollama.rs を共通関数利用に置換**

`src-tauri/src/classifier/ollama.rs` から `fn extract_json`(30-38) と `pub fn parse_response`(40-51) を削除。呼び出し箇所（`classify` 内 139行目付近）の `Self::parse_response(&content)` を `crate::classifier::parse::parse_classify_result(&content)` に変更。`ollama.rs` の `#[cfg(test)] mod tests` から `test_extract_json*` と `test_parse_response*` を削除（parse.rs へ移設済みのため）。ファイル冒頭の `use` から不要になった `serde` 由来の未使用 import が出たら削除。

- [ ] **Step 5: 全体テストが通ることを確認**

Run: `cd src-tauri && cargo test classifier`
Expected: PASS（重複テスト削除後も全緑）

- [ ] **Step 6: コミット**

```bash
git add src-tauri/src/classifier/parse.rs src-tauri/src/classifier/mod.rs src-tauri/src/classifier/ollama.rs
git commit -m "refactor(classifier): JSON抽出/パースを共通 parse モジュールへ集約"
```

---

## Task 4: trait 統合（TextGenerator を LlmClassifier のスーパートレイトに）

**Files:**
- Modify: `src-tauri/src/classifier/mod.rs`

**Interfaces:**
- Produces: `pub trait LlmClassifier: TextGenerator + Send + Sync { ... }`（`TextGenerator` は従来どおり）

- [ ] **Step 1: mod.rs の trait 定義を変更**

`src-tauri/src/classifier/mod.rs` の trait 定義を、`TextGenerator` を先に定義してから `LlmClassifier` がそれを継承する形へ:

```rust
/// 汎用テキスト生成（ダイジェスト生成等）。全プロバイダが実装する。
#[async_trait]
pub trait TextGenerator: Send + Sync {
    async fn generate_text(
        &self,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String, AppError>;
}

#[async_trait]
pub trait LlmClassifier: TextGenerator + Send + Sync {
    async fn classify(
        &self,
        mail: &MailSummary,
        projects: &[ProjectSummary],
        corrections: &[CorrectionEntry],
    ) -> Result<ClassifyResult, AppError>;

    async fn health_check(&self) -> Result<(), AppError>;
}
```

（`OllamaClassifier` は既に両 trait を実装しているため変更不要。）

- [ ] **Step 2: コンパイル/テスト確認**

Run: `cd src-tauri && cargo test classifier`
Expected: PASS

- [ ] **Step 3: コミット**

```bash
git add src-tauri/src/classifier/mod.rs
git commit -m "refactor(classifier): TextGenerator を LlmClassifier のスーパートレイトに統合"
```

---

## Task 5: ClaudeClassifier の実装

**Files:**
- Create: `src-tauri/src/classifier/claude.rs`
- Modify: `src-tauri/src/classifier/mod.rs`（`pub mod claude;`）
- Test: `claude.rs` 内 `#[cfg(test)]`（リクエストボディ構築・レスポンス抽出のユニット、HTTP実通信はしない）

**Interfaces:**
- Consumes: `crate::classifier::parse::parse_classify_result`, `crate::classifier::prompt`, trait `LlmClassifier`/`TextGenerator`
- Produces: `pub struct ClaudeClassifier`; `pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Result<Self, AppError>`

- [ ] **Step 1: 失敗するテストを書く（レスポンス本文抽出）**

`src-tauri/src/classifier/claude.rs`:

```rust
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::classifier::{parse, prompt, LlmClassifier, TextGenerator};
use crate::error::AppError;
use crate::models::classifier::{
    ClassifyAction, ClassifyResult, CorrectionEntry, MailSummary, ProjectSummary,
};

const ANTHROPIC_MESSAGES_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_MODELS_URL: &str = "https://api.anthropic.com/v1/models";
const ANTHROPIC_VERSION: &str = "2023-06-01";
const MAX_TOKENS: u32 = 1024;

pub struct ClaudeClassifier {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl ClaudeClassifier {
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Result<Self, AppError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| AppError::HttpRequest(e.to_string()))?;
        Ok(Self {
            api_key: api_key.into(),
            model: model.into(),
            client,
        })
    }

    fn build_request(&self, system_prompt: &str, user_prompt: &str) -> MessagesRequest {
        MessagesRequest {
            model: self.model.clone(),
            max_tokens: MAX_TOKENS,
            system: system_prompt.to_string(),
            messages: vec![MessageParam {
                role: "user".to_string(),
                content: user_prompt.to_string(),
            }],
        }
    }

    /// レスポンス JSON から最初の text ブロックを取り出す。
    fn extract_text(resp: &MessagesResponse) -> Result<String, AppError> {
        resp.content
            .iter()
            .find_map(|b| {
                if b.block_type == "text" {
                    b.text.clone()
                } else {
                    None
                }
            })
            .ok_or_else(|| AppError::InvalidLlmResponse("no text block in response".to_string()))
    }

    async fn chat(&self, system_prompt: &str, user_prompt: &str) -> Result<String, AppError> {
        let body = self.build_request(system_prompt, user_prompt);
        let response = self
            .client
            .post(ANTHROPIC_MESSAGES_URL)
            .header("content-type", "application/json")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::HttpRequest(e.to_string()))?;

        if !response.status().is_success() {
            return Err(AppError::Classifier(format!(
                "Anthropic API returned status {}",
                response.status()
            )));
        }

        let parsed: MessagesResponse = response
            .json()
            .await
            .map_err(|e| AppError::InvalidLlmResponse(e.to_string()))?;
        Self::extract_text(&parsed)
    }
}

#[derive(Debug, Serialize)]
struct MessagesRequest {
    model: String,
    max_tokens: u32,
    system: String,
    messages: Vec<MessageParam>,
}

#[derive(Debug, Serialize)]
struct MessageParam {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct MessagesResponse {
    content: Vec<ContentBlock>,
}

#[derive(Debug, Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    text: Option<String>,
}

#[async_trait]
impl TextGenerator for ClaudeClassifier {
    async fn generate_text(
        &self,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String, AppError> {
        self.chat(system_prompt, user_prompt).await
    }
}

#[async_trait]
impl LlmClassifier for ClaudeClassifier {
    async fn classify(
        &self,
        mail: &MailSummary,
        projects: &[ProjectSummary],
        corrections: &[CorrectionEntry],
    ) -> Result<ClassifyResult, AppError> {
        let user_prompt = prompt::build_user_prompt(mail, projects, corrections);
        let content = self.chat(prompt::SYSTEM_PROMPT, &user_prompt).await?;
        match parse::parse_classify_result(&content) {
            Ok(result) => Ok(result),
            Err(_) => Ok(ClassifyResult {
                action: ClassifyAction::Unclassified,
                confidence: 0.0,
                reason: format!(
                    "LLMの応答を解析できませんでした。生の応答: {}",
                    &content[..content.len().min(100)]
                ),
            }),
        }
    }

    async fn health_check(&self) -> Result<(), AppError> {
        let response = self
            .client
            .get(ANTHROPIC_MODELS_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .send()
            .await
            .map_err(|e| AppError::HttpRequest(e.to_string()))?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(AppError::Classifier(format!(
                "Anthropic health check failed with status {}",
                response.status()
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_request_shape() {
        let c = ClaudeClassifier::new("sk-test", "claude-haiku-4-5").unwrap();
        let req = c.build_request("sys", "usr");
        assert_eq!(req.model, "claude-haiku-4-5");
        assert_eq!(req.max_tokens, MAX_TOKENS);
        assert_eq!(req.system, "sys");
        assert_eq!(req.messages.len(), 1);
        assert_eq!(req.messages[0].role, "user");
        assert_eq!(req.messages[0].content, "usr");
    }

    #[test]
    fn test_extract_text_finds_text_block() {
        let resp = MessagesResponse {
            content: vec![ContentBlock {
                block_type: "text".to_string(),
                text: Some("{\"action\":\"unclassified\"}".to_string()),
            }],
        };
        assert_eq!(
            ClaudeClassifier::extract_text(&resp).unwrap(),
            "{\"action\":\"unclassified\"}"
        );
    }

    #[test]
    fn test_extract_text_no_text_block_errs() {
        let resp = MessagesResponse {
            content: vec![ContentBlock {
                block_type: "tool_use".to_string(),
                text: None,
            }],
        };
        assert!(ClaudeClassifier::extract_text(&resp).is_err());
    }

    #[test]
    fn test_response_deserializes_from_api_json() {
        let json = r#"{"content":[{"type":"text","text":"hello"}]}"#;
        let resp: MessagesResponse = serde_json::from_str(json).unwrap();
        assert_eq!(ClaudeClassifier::extract_text(&resp).unwrap(), "hello");
    }
}
```

- [ ] **Step 2: mod.rs に登録**

`src-tauri/src/classifier/mod.rs` の mod 群に `pub mod claude;` を追加（`pub mod claude;` を `pub mod ollama;` の前、アルファベット順）。

- [ ] **Step 3: テストが失敗→通ることを確認**

Run: `cd src-tauri && cargo test classifier::claude`
Expected: PASS（初回コンパイル後、ユニットは通る）

- [ ] **Step 4: コミット**

```bash
git add src-tauri/src/classifier/claude.rs src-tauri/src/classifier/mod.rs
git commit -m "feat(classifier): Claude Messages API 対応の ClaudeClassifier を追加"
```

---

## Task 6: ファクトリ build_classifier

**Files:**
- Create: `src-tauri/src/classifier/factory.rs`
- Modify: `src-tauri/src/classifier/mod.rs`（`pub mod factory;`）
- Test: `factory.rs` 内 `#[cfg(test)]`

**Interfaces:**
- Consumes: `settings::get_or_default`, `SecureStore::get`, `OllamaClassifier::new`, `ClaudeClassifier::new`
- Produces: `pub fn build_classifier(conn: &rusqlite::Connection, secure_store: &crate::secure_store::SecureStore) -> Result<Box<dyn LlmClassifier>, AppError>`

- [ ] **Step 1: 失敗するテストを書く**

`src-tauri/src/classifier/factory.rs`:

```rust
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
    use tempfile::TempDir;

    fn setup() -> (Connection, SecureStore, TempDir) {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        let dir = TempDir::new().unwrap();
        let store =
            SecureStore::new(dir.path().join("test.stronghold"), b"test-password-123").unwrap();
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
        let err = build_classifier(&conn, &store).unwrap_err();
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
        let err = build_classifier(&conn, &store).unwrap_err();
        assert!(matches!(err, AppError::MissingApiKey(_)));
    }

    #[test]
    fn test_openai_errs_unsupported() {
        let (conn, store, _d) = setup();
        settings::set(&conn, "llm_provider", "openai").unwrap();
        let err = build_classifier(&conn, &store).unwrap_err();
        assert!(matches!(err, AppError::UnsupportedProvider(_)));
    }
}
```

- [ ] **Step 2: mod.rs に登録**

`src-tauri/src/classifier/mod.rs` に `pub mod factory;` を追加（`pub mod claude;` の後）。

- [ ] **Step 3: tempfile が dev-dependencies にあるか確認**

Run: `cd src-tauri && grep -n "tempfile" Cargo.toml`
Expected: `[dev-dependencies]` に `tempfile` がある。無ければ追加:
```bash
cd src-tauri && cargo add --dev tempfile
```
（`account_commands.rs` のテストが `dir.path()` を使っているので既にあるはず。）

- [ ] **Step 4: テストが通ることを確認**

Run: `cd src-tauri && cargo test classifier::factory`
Expected: PASS

- [ ] **Step 5: コミット**

```bash
git add src-tauri/src/classifier/factory.rs src-tauri/src/classifier/mod.rs src-tauri/Cargo.toml
git commit -m "feat(classifier): プロバイダ判定を集約する build_classifier ファクトリを追加"
```

---

## Task 7: 呼び出し側をファクトリへ置換（classify/directory/lib）

3ファイル4箇所の直接構築を `build_classifier` に置き換える。各コマンドは `SecureStore` state を受け取る。

**Files:**
- Modify: `src-tauri/src/commands/classify_commands.rs`（`classify_mail` 42-61、`classify_unassigned` 96-118）
- Modify: `src-tauri/src/commands/directory_commands.rs`（`rescan_project_directory` 64-79）
- Modify: `src-tauri/src/lib.rs`（起動時スキャン 101-116）

**Interfaces:**
- Consumes: `crate::classifier::factory::build_classifier`, `crate::state::SecureStoreState`
- Produces: 挙動不変（構築だけ差し替え）。`Box<dyn LlmClassifier>` は `&*classifier` で `&dyn TextGenerator`/`&dyn LlmClassifier` として使う。

- [ ] **Step 1: classify_commands.rs を置換**

`classify_mail`（シグネチャに `secure_store: State<'_, SecureStoreState>` を追加）:
- import を差し替え: `use crate::classifier::ollama::OllamaClassifier;` を削除し `use crate::classifier::factory::build_classifier;` を追加。`use crate::state::{DbState, SecureStoreState};` に拡張。
- ロックブロック 49-58 から `endpoint`/`model` の取得を削除し、代わりにロック内で classifier を構築:

```rust
    let (mail, project_summaries, corrections, classifier) = {
        let conn = db.0.lock().map_err(AppError::lock_err)?;
        let mail = mails::get_mail_by_id(&conn, &mail_id)?;
        let project_summaries = projects::build_project_summaries(&conn, &mail.account_id, false)?;
        let corrections =
            assignments::get_recent_corrections(&conn, &mail.account_id, 20).unwrap_or_default();
        let classifier = build_classifier(&conn, &secure_store.0)?;
        (mail, project_summaries, corrections, classifier)
    };

    let mail_summary = MailSummary::from_mail(&mail);
```

（61行目の `let classifier = OllamaClassifier::new(...)?;` は削除。以降 `classifier.health_check()` / `classifier.classify(...)` はそのまま動く。）

`classify_unassigned`（シグネチャに `secure_store: State<'_, SecureStoreState>` を追加）同様に 108-118 を:

```rust
    let (mails, corrections, classifier) = {
        let conn = db.0.lock().map_err(AppError::lock_err)?;
        let mails = assignments::get_unclassified_mails(&conn, &account_id)?;
        let corrections =
            assignments::get_recent_corrections(&conn, &account_id, 20).unwrap_or_default();
        let classifier = build_classifier(&conn, &secure_store.0)?;
        (mails, corrections, classifier)
    };
```

（118行目の `OllamaClassifier::new` 行は削除。）

- [ ] **Step 2: directory_commands.rs を置換**

`rescan_project_directory`（シグネチャに `secure_store: State<'_, SecureStoreState>` を追加）:
- import: `use crate::classifier::ollama::OllamaClassifier;` を `use crate::classifier::factory::build_classifier;` に、`use crate::state::DbState;` を `use crate::state::{DbState, SecureStoreState};` に。
- 69-79 を:

```rust
    let classifier = {
        let conn = db.0.lock().map_err(AppError::lock_err)?;
        build_classifier(&conn, &secure_store.0)?
    };
    // プロバイダが Claude のときのみクラウド送信になる。cloud フラグは
    // 送信可否ポリシー適用のためのもので、build_classifier とは独立。
    let cloud = {
        let conn = db.0.lock().map_err(AppError::lock_err)?;
        crate::db::settings::get_or_default(&conn, "llm_provider", "ollama") == "claude"
    };
    project_context::rescan_project(&db.0, classifier.as_ref(), &project_id, cloud).await
```

（`rescan_project` は `&dyn TextGenerator` を取る。`classifier.as_ref()` は `&dyn LlmClassifier`。`LlmClassifier: TextGenerator` なので upcast が必要な場合は `let gen: &dyn TextGenerator = classifier.as_ref();` を挟む。Rust の trait upcasting が使えない toolchain では `project_context::rescan_project` の引数を `&dyn LlmClassifier` に緩める案があるが、まず `as_ref()` でビルドを試し、失敗したら次の代替へ。）

**代替（trait upcast 不可の場合）:** `rescan_project` の引数型を `generator: &dyn TextGenerator` のまま維持し、`ClaudeClassifier`/`OllamaClassifier` を直接 `Box<dyn TextGenerator>` として渡せるよう、factory に姉妹関数は作らず、ここで `classifier.as_ref() as &dyn TextGenerator` と明示キャストする。

- [ ] **Step 3: lib.rs 起動時スキャンを置換**

`src-tauri/src/lib.rs` の 101-116（`(endpoint, model)` 取得〜`OllamaClassifier::new`）を、`db` と同様に `secure_store` state を取得して置換:

```rust
                    let secure_store = app_handle.state::<SecureStoreState>();
                    let classifier = {
                        let conn = match db.0.lock() {
                            Ok(c) => c,
                            Err(_) => return,
                        };
                        match classifier::factory::build_classifier(&conn, &secure_store.0) {
                            Ok(c) => c,
                            Err(_) => return,
                        }
                    };
                    for project_id in targets {
                        if let Err(e) = project_context::rescan_project(
                            &db.0, classifier.as_ref(), &project_id, false,
                        )
                        .await
                        {
                            eprintln!("[warn] startup scan failed for {}: {}", project_id, e);
                        }
                    }
```

（`use state::SecureStoreState;` は 19行目で既に import 済み。`app_handle.state::<SecureStoreState>()` が使える。起動時スキャンの `cloud` は従来どおり `false` 固定でよい＝コンテキスト送信は明示許可時のみポリシー側で制御される。）

- [ ] **Step 4: ビルド**

Run: `cd src-tauri && cargo build`
Expected: 成功。trait upcast エラーが出たら Step 2 の代替に沿って明示キャストを入れる。

- [ ] **Step 5: 既存テスト＋クリッピー**

Run: `cd src-tauri && cargo test && cargo clippy -- -D warnings`
Expected: PASS

- [ ] **Step 6: コミット**

```bash
git add src-tauri/src/commands/classify_commands.rs src-tauri/src/commands/directory_commands.rs src-tauri/src/lib.rs
git commit -m "refactor(classifier): Classifier 構築を全呼び出し側で build_classifier に統一"
```

---

## Task 8: LlmSettings モデルと設定コマンド

**Files:**
- Create: `src-tauri/src/models/settings.rs`
- Modify: `src-tauri/src/models/mod.rs`（`pub mod settings;`）
- Create: `src-tauri/src/commands/settings_commands.rs`
- Modify: `src-tauri/src/commands/mod.rs`（`pub mod settings_commands;`）
- Modify: `src-tauri/src/lib.rs`（`invoke_handler` に3コマンド登録）
- Test: `settings_commands.rs` 内 `#[cfg(test)]`（内部ヘルパー `load_llm_settings`/`store_llm_settings` を conn+store 直渡しでテスト）

**Interfaces:**
- Produces:
  - `LlmSettings { provider, ollama_endpoint, ollama_model, claude_model, claude_api_key_set }`
  - コマンド: `get_llm_settings`, `set_llm_settings`, `test_llm_connection`
  - 内部: `pub(crate) fn load_llm_settings(conn, store) -> Result<LlmSettings, AppError>`、`pub(crate) fn store_llm_settings(conn, store, provider, ollama_endpoint, ollama_model, claude_model, claude_api_key: Option<String>) -> Result<(), AppError>`

- [ ] **Step 1: モデル定義**

`src-tauri/src/models/settings.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LlmSettings {
    pub provider: String,
    pub ollama_endpoint: String,
    pub ollama_model: String,
    pub claude_model: String,
    /// APIキー本体は返さない。登録済みかどうかのみ。
    pub claude_api_key_set: bool,
}
```

`src-tauri/src/models/mod.rs` に `pub mod settings;` を追加。

- [ ] **Step 2: 失敗するテストを書く（load/store の往復）**

`src-tauri/src/commands/settings_commands.rs`:

```rust
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
    use tempfile::TempDir;

    fn setup() -> (Connection, SecureStore, TempDir) {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        let dir = TempDir::new().unwrap();
        let store =
            SecureStore::new(dir.path().join("t.stronghold"), b"pw-123456").unwrap();
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
```

- [ ] **Step 3: mod.rs 登録**

`src-tauri/src/commands/mod.rs` に `pub mod settings_commands;` を追加。

- [ ] **Step 4: テスト**

Run: `cd src-tauri && cargo test settings_commands`
Expected: PASS

- [ ] **Step 5: lib.rs の invoke_handler に登録**

`src-tauri/src/lib.rs` の `tauri::generate_handler![ ... ]` 内（既存コマンド群の末尾）に追加:

```rust
            commands::settings_commands::get_llm_settings,
            commands::settings_commands::set_llm_settings,
            commands::settings_commands::test_llm_connection,
```

- [ ] **Step 6: ビルド確認**

Run: `cd src-tauri && cargo build`
Expected: 成功

- [ ] **Step 7: コミット**

```bash
git add src-tauri/src/models/settings.rs src-tauri/src/models/mod.rs src-tauri/src/commands/settings_commands.rs src-tauri/src/commands/mod.rs src-tauri/src/lib.rs
git commit -m "feat(settings): LLM設定の取得/保存/接続テスト用コマンドを追加"
```

---

## Task 9: フロント型定義

**Files:**
- Create: `src/types/settings.ts`

**Interfaces:**
- Produces: `LlmProvider`, `LlmSettings`（Rust の `LlmSettings` と一致）

- [ ] **Step 1: 型定義**

`src/types/settings.ts`:

```typescript
export type LlmProvider = "ollama" | "claude" | "openai";

export interface LlmSettings {
  provider: LlmProvider;
  ollama_endpoint: string;
  ollama_model: string;
  claude_model: string;
  claude_api_key_set: boolean;
}
```

- [ ] **Step 2: 型チェック**

Run: `pnpm tsc --noEmit`
Expected: エラーなし

- [ ] **Step 3: コミット**

```bash
git add src/types/settings.ts
git commit -m "feat(ui): LLM設定のTypeScript型を追加"
```

---

## Task 10: LlmSettingsDialog コンポーネント（テスト先行）

**Files:**
- Create: `src/components/sidebar/LlmSettingsDialog.tsx`
- Create: `src/__tests__/LlmSettingsDialog.test.tsx`

**Interfaces:**
- Consumes: `invoke<LlmSettings>("get_llm_settings")`, `invoke("set_llm_settings", {...})`, `invoke("test_llm_connection")`, `useErrorStore`
- Produces: `export function LlmSettingsDialog({ onClose }: { onClose: () => void })`

- [ ] **Step 1: 失敗するテストを書く**

`src/__tests__/LlmSettingsDialog.test.tsx`:

```tsx
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import { LlmSettingsDialog } from "../components/sidebar/LlmSettingsDialog";

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

const baseSettings = {
  provider: "ollama",
  ollama_endpoint: "http://localhost:11434",
  ollama_model: "llama3.1:8b",
  claude_model: "claude-haiku-4-5",
  claude_api_key_set: false,
};

beforeEach(() => {
  invokeMock.mockReset();
  invokeMock.mockImplementation((cmd: string) => {
    if (cmd === "get_llm_settings") return Promise.resolve(baseSettings);
    return Promise.resolve();
  });
});

describe("LlmSettingsDialog", () => {
  it("初期表示で現在のプロバイダを読み込む", async () => {
    render(<LlmSettingsDialog onClose={() => {}} />);
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("get_llm_settings"));
    expect(screen.getByLabelText("Ollama（ローカル）")).toBeChecked();
  });

  it("Claudeを選ぶと警告バナーとAPIキー入力が出る", async () => {
    render(<LlmSettingsDialog onClose={() => {}} />);
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("get_llm_settings"));
    fireEvent.click(screen.getByLabelText("Claude API"));
    expect(screen.getByText(/クラウドLLMを使用します/)).toBeInTheDocument();
    expect(screen.getByLabelText("Claude APIキー")).toBeInTheDocument();
  });

  it("ChatGPTは選択できない（disabled）", async () => {
    render(<LlmSettingsDialog onClose={() => {}} />);
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("get_llm_settings"));
    expect(screen.getByLabelText(/ChatGPT/)).toBeDisabled();
  });

  it("接続テストボタンで test_llm_connection を呼ぶ", async () => {
    render(<LlmSettingsDialog onClose={() => {}} />);
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("get_llm_settings"));
    fireEvent.click(screen.getByRole("button", { name: "接続テスト" }));
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("test_llm_connection"),
    );
  });
});
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `pnpm vitest run src/__tests__/LlmSettingsDialog.test.tsx`
Expected: FAIL（コンポーネント未実装）

- [ ] **Step 3: コンポーネント実装**

`src/components/sidebar/LlmSettingsDialog.tsx`:

```tsx
import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { LlmProvider, LlmSettings } from "../../types/settings";
import { useErrorStore } from "../../stores/errorStore";

interface Props {
  onClose: () => void;
}

const PROVIDERS: { value: LlmProvider; label: string; disabled?: boolean }[] = [
  { value: "ollama", label: "Ollama（ローカル）" },
  { value: "claude", label: "Claude API" },
  { value: "openai", label: "ChatGPT（未対応・今後対応予定）", disabled: true },
];

export function LlmSettingsDialog({ onClose }: Props) {
  const [settings, setSettings] = useState<LlmSettings | null>(null);
  const [apiKeyInput, setApiKeyInput] = useState("");
  const [testResult, setTestResult] = useState<string | null>(null);

  useEffect(() => {
    void (async () => {
      try {
        const s = await invoke<LlmSettings>("get_llm_settings");
        setSettings(s);
      } catch (e) {
        useErrorStore.getState().addError(String(e));
      }
    })();
  }, []);

  const update = useCallback(
    <K extends keyof LlmSettings>(key: K, value: LlmSettings[K]) => {
      setSettings((prev) => (prev ? { ...prev, [key]: value } : prev));
    },
    [],
  );

  const handleSave = async () => {
    if (!settings) return;
    try {
      await invoke("set_llm_settings", {
        provider: settings.provider,
        ollamaEndpoint: settings.ollama_endpoint,
        ollamaModel: settings.ollama_model,
        claudeModel: settings.claude_model,
        claudeApiKey: apiKeyInput === "" ? null : apiKeyInput,
      });
      onClose();
    } catch (e) {
      useErrorStore.getState().addError(String(e));
    }
  };

  const handleTest = async () => {
    setTestResult(null);
    try {
      await invoke("test_llm_connection");
      setTestResult("接続成功");
    } catch (e) {
      setTestResult(`接続失敗: ${String(e)}`);
    }
  };

  if (!settings) {
    return (
      <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40">
        <div className="rounded-lg bg-white px-6 py-4 text-sm text-gray-500">
          読み込み中…
        </div>
      </div>
    );
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40">
      <div className="flex max-h-[80vh] w-[520px] flex-col rounded-lg bg-white shadow-xl">
        <div className="border-b px-5 py-3">
          <h2 className="text-sm font-bold">LLM設定</h2>
        </div>
        <div className="flex-1 space-y-4 overflow-y-auto px-5 py-4">
          <fieldset className="space-y-2">
            <legend className="text-xs font-semibold uppercase tracking-wide text-gray-400">
              プロバイダ
            </legend>
            {PROVIDERS.map((p) => (
              <label key={p.value} className="flex items-center gap-2 text-sm">
                <input
                  type="radio"
                  name="llm-provider"
                  aria-label={p.label}
                  checked={settings.provider === p.value}
                  disabled={p.disabled}
                  onChange={() => update("provider", p.value)}
                />
                <span className={p.disabled ? "text-gray-400" : ""}>{p.label}</span>
              </label>
            ))}
          </fieldset>

          {settings.provider === "ollama" && (
            <div className="space-y-2">
              <label className="block text-sm">
                エンドポイント
                <input
                  className="mt-1 w-full rounded border px-2 py-1 text-sm"
                  value={settings.ollama_endpoint}
                  onChange={(e) => update("ollama_endpoint", e.target.value)}
                />
              </label>
              <label className="block text-sm">
                モデル
                <input
                  className="mt-1 w-full rounded border px-2 py-1 text-sm"
                  value={settings.ollama_model}
                  onChange={(e) => update("ollama_model", e.target.value)}
                />
              </label>
            </div>
          )}

          {settings.provider === "claude" && (
            <div className="space-y-2">
              <p className="rounded bg-amber-50 px-3 py-2 text-xs text-amber-700">
                クラウドLLMを使用します。件名・送信者・本文冒頭300文字と、許可した案件コンテキストが
                Anthropic に送信されます。
              </p>
              <label className="block text-sm">
                Claude APIキー
                <input
                  type="password"
                  aria-label="Claude APIキー"
                  className="mt-1 w-full rounded border px-2 py-1 text-sm"
                  placeholder={
                    settings.claude_api_key_set ? "••••••••（登録済み・変更時のみ入力）" : "sk-ant-..."
                  }
                  value={apiKeyInput}
                  onChange={(e) => setApiKeyInput(e.target.value)}
                />
              </label>
              <label className="block text-sm">
                モデル
                <input
                  className="mt-1 w-full rounded border px-2 py-1 text-sm"
                  placeholder="claude-haiku-4-5"
                  value={settings.claude_model}
                  onChange={(e) => update("claude_model", e.target.value)}
                />
              </label>
            </div>
          )}

          <div className="flex items-center gap-3">
            <button
              onClick={() => void handleTest()}
              className="rounded border border-gray-300 px-3 py-1.5 text-sm hover:bg-gray-50"
            >
              接続テスト
            </button>
            {testResult && (
              <span className="text-xs text-gray-600">{testResult}</span>
            )}
          </div>
        </div>
        <div className="flex justify-end gap-2 border-t px-5 py-3">
          <button
            onClick={onClose}
            className="rounded px-4 py-1.5 text-sm text-gray-600 hover:bg-gray-100"
          >
            キャンセル
          </button>
          <button
            onClick={() => void handleSave()}
            className="rounded bg-blue-600 px-4 py-1.5 text-sm font-medium text-white hover:bg-blue-700"
          >
            保存
          </button>
        </div>
      </div>
    </div>
  );
}
```

- [ ] **Step 4: テストが通ることを確認**

Run: `pnpm vitest run src/__tests__/LlmSettingsDialog.test.tsx`
Expected: PASS

- [ ] **Step 5: 型チェック**

Run: `pnpm tsc --noEmit`
Expected: エラーなし

- [ ] **Step 6: コミット**

```bash
git add src/components/sidebar/LlmSettingsDialog.tsx src/__tests__/LlmSettingsDialog.test.tsx
git commit -m "feat(ui): LLMプロバイダ選択ダイアログを追加"
```

---

## Task 11: サイドバーに設定ボタンを追加してダイアログを開く

**Files:**
- Modify: サイドバーのルートコンポーネント（実装時に `src/components/sidebar/` 内で特定。案件ツリーの見出し付近に歯車ボタンを置く）

**Interfaces:**
- Consumes: `LlmSettingsDialog`
- Produces: 設定ボタン押下で `LlmSettingsDialog` を開く UI

- [ ] **Step 1: サイドバールートを特定**

Run: `ls src/components/sidebar/ && grep -rln "案件\|projects\|Sidebar" src/components/sidebar/`
サイドバー最上位のコンポーネント（ヘッダ/ツールバーを持つもの）を対象にする。

- [ ] **Step 2: 状態とボタンを追加**

対象コンポーネントに以下を追加:
- `import { useState } from "react";`（既存なら不要）と `import { LlmSettingsDialog } from "./LlmSettingsDialog";`
- `const [showLlmSettings, setShowLlmSettings] = useState(false);`
- ヘッダ付近に歯車ボタン:
  ```tsx
  <button
    onClick={() => setShowLlmSettings(true)}
    className="rounded p-1 text-gray-500 hover:bg-gray-100"
    aria-label="LLM設定を開く"
    title="LLM設定"
  >
    ⚙️
  </button>
  ```
- コンポーネント末尾（return 内の適切な場所）に:
  ```tsx
  {showLlmSettings && (
    <LlmSettingsDialog onClose={() => setShowLlmSettings(false)} />
  )}
  ```

- [ ] **Step 3: 型チェック＋既存テスト**

Run: `pnpm tsc --noEmit && pnpm vitest run`
Expected: エラーなし・全緑

- [ ] **Step 4: 手動起動確認（任意）**

Run: `pnpm tauri dev`
確認: サイドバーの歯車ボタン → ダイアログが開き、プロバイダ切替・保存・接続テストが動作する。

- [ ] **Step 5: コミット**

```bash
git add src/components/sidebar/
git commit -m "feat(ui): サイドバーにLLM設定ボタンを追加"
```

---

## Task 12: 全体検証と設計書ステータス更新

**Files:**
- Modify: `docs/superpowers/specs/2026-07-10-llm-provider-selection-design.md`（ステータスを実装済みに）

- [ ] **Step 1: Rust 全テスト＋clippy**

Run: `cd src-tauri && cargo test && cargo clippy -- -D warnings`
Expected: 全緑・警告なし

- [ ] **Step 2: フロント全テスト＋型チェック＋lint**

Run: `pnpm tsc --noEmit && pnpm vitest run && pnpm lint`
Expected: 全緑

- [ ] **Step 3: 設計書のステータス更新**

`docs/superpowers/specs/2026-07-10-llm-provider-selection-design.md` のステータス行を `承認済み（実装前）` → `実装済み（Ollama + Claude）` に更新。

- [ ] **Step 4: コミット**

```bash
git add docs/superpowers/specs/2026-07-10-llm-provider-selection-design.md
git commit -m "docs(specs): LLMプロバイダ選択の設計書ステータスを実装済みに更新"
```
```
```
