# LLMプロバイダ選択設定 設計書

- 作成日: 2026-07-10
- ステータス: 実装済み（Ollama + Claude）／ Claude (Vertex AI) 追加中
- 関連: `2026-04-12-pigeon-design.md`, `2026-04-13-phase2-ai-classification-design.md`, `2026-07-09-project-directory-context-design.md`

## 1. 目的

設定ウィンドウ（モーダルダイアログ）を追加し、アプリが使用するLLMプロバイダをユーザーが選択できるようにする。

- **ローカル**: Ollama（現状のデフォルト、`llama3.1:8b`）
- **クラウド (Anthropic 直)**: Claude API（実装済み、デフォルト `claude-haiku-4-5`）
- **クラウド (GCP)**: Claude on Vertex AI / Agent Platform（新規追加。Anthropic 直の Claude とは別プロバイダ `claude_vertex`。§12 参照）
- **将来**: ChatGPT（UIの口だけ用意し、選択すると「未対応」表示。実装は次フェーズ）

プロバイダは**アプリ全体で単一**とする。分類・ダイジェスト生成など、すべての用途で選択したプロバイダを使う。

## 2. 背景と現状の課題

現状 `OllamaClassifier::new(&endpoint, &model)` が **3ファイル・4箇所**で直接構築されている:

- `src-tauri/src/lib.rs`（起動時スキャン）
- `src-tauri/src/commands/classify_commands.rs`（2箇所: classify_mail / classify_unassigned）
- `src-tauri/src/commands/directory_commands.rs`（再スキャン）

プロバイダ選択を導入すると、この分散した構築ロジックが破綻する。よって**構築ロジックをファクトリ関数に集約する**リファクタリングを本作業の中核とする。既存の `LlmClassifier` / `TextGenerator` trait はすでに抽象化されているため、これを活かす。

これは agent.md「不具合修正方針（負債を残しにくくする）」に沿い、その場しのぎの分岐追加ではなく構造の健全化を選ぶ判断である。

## 3. アーキテクチャ

```
settings(DB, 平文)  +  SecureStore(Stronghold, 暗号化: APIキー)
        │
        ▼
classifier::factory::build_classifier(conn, secure_store)
        │  → llm_provider を読みプロバイダを判定
        ▼
Box<dyn LlmClassifier>          ← LlmClassifier: TextGenerator + Send + Sync
        │
        ├─ OllamaClassifier   (既存)
        └─ ClaudeClassifier   (新規)
```

### 3.1 trait の統合

現状 `LlmClassifier` と `TextGenerator` は独立した2つの trait。ファクトリが単一の trait object を返して両用途で使えるよう、`TextGenerator` を `LlmClassifier` のスーパートレイトにまとめる:

```rust
#[async_trait]
pub trait LlmClassifier: TextGenerator + Send + Sync {
    async fn classify(...) -> Result<ClassifyResult, AppError>;
    async fn health_check(&self) -> Result<(), AppError>;
}
```

これにより `Box<dyn LlmClassifier>` 1つで `classify` も `generate_text` も呼べる。`project_context` のダイジェスト生成（`generate_text` 利用）もこの型で受けられる。

### 3.2 JSON抽出ロジックの共通化

現在 `OllamaClassifier::extract_json` / `parse_response` が Ollama 側にある。Claude も同じ「本文からJSONオブジェクトを取り出してパース」する必要があるため、`classifier` モジュール直下の共通関数（例: `classifier::parse::parse_classify_result`）へ引き上げ、Ollama/Claude 両方から使う。既存テストも共通側へ移設する。

### 3.3 ファクトリのロジック

```
build_classifier(conn, secure_store):
  provider = settings::get_or_default(conn, "llm_provider", "ollama")
  match provider:
    "ollama" →
      endpoint = get_or_default("ollama_endpoint", "http://localhost:11434")
      model    = get_or_default("ollama_model", "llama3.1:8b")
      Ok(Box::new(OllamaClassifier::new(endpoint, model)?))
    "claude" →
      api_key = secure_store.get("claude_api_key")?
        └─ None または空 → Err(AppError::MissingApiKey)
      model = get_or_default("claude_model", "claude-haiku-4-5")
      Ok(Box::new(ClaudeClassifier::new(api_key, model)?))
    "openai" → Err(AppError::UnsupportedProvider("openai"))
    _        → Err(AppError::UnsupportedProvider(provider))
```

**フォールバックはしない。** Claude選択中にキー未設定なら黙ってOllamaに落とさず、明示的にエラーを返す。ユーザーの意図（クラウドを使う）と実挙動が食い違う状態を防ぐため。エラーは既存 `useErrorStore` 経由でUIに表示される。

## 4. データ設計

秘密情報のみ Stronghold、それ以外は settings テーブルという既存の切り分けを踏襲する。

### 4.1 settings テーブル（key-value, 平文）

| キー | 既存/新規 | デフォルト | 説明 |
|---|---|---|---|
| `llm_provider` | 新規 | `ollama` | `ollama` / `claude` / `openai` |
| `ollama_endpoint` | 既存 | `http://localhost:11434` | Ollamaエンドポイント |
| `ollama_model` | 既存 | `llama3.1:8b` | Ollamaモデル |
| `claude_model` | 新規 | `claude-haiku-4-5` | Claudeモデル（自由入力で上書き） |
| `openai_model` | 新規 | `gpt-4o` | 将来用（口だけ） |

`settings.rs` は現在 `get_or_default` / `get_u32_or` のみで書き込み関数が無い。以下を新規追加する:

```rust
/// key に value を UPSERT する。
pub fn set(conn: &Connection, key: &str, value: &str) -> Result<(), AppError>;
```

### 4.2 SecureStore（Stronghold, 暗号化）

| キー | 説明 |
|---|---|
| `claude_api_key` | Claude APIキー |
| `openai_api_key` | 将来用 |

APIキーを settings（平文DB）に置かないのは agent.md セキュリティルール（キー・トークンはOSキーチェーン相当に保存、SQLiteに平文で保存しない）に沿うため。Pigeon では「OSキーチェーン相当」＝ Stronghold（`SecureStore`）である。

## 5. Claude API 連携（ClaudeClassifier）

公式ドキュメント（2026-07-10確認）に基づく:

- エンドポイント: `POST https://api.anthropic.com/v1/messages`
- 必須ヘッダ:
  - `content-type: application/json`
  - `x-api-key: <APIキー>`
  - `anthropic-version: 2023-06-01`
- リクエストボディ（最小）:
  ```json
  {
    "model": "claude-haiku-4-5",
    "max_tokens": 1024,
    "system": "<システムプロンプト>",
    "messages": [{ "role": "user", "content": "<ユーザープロンプト>" }]
  }
  ```
- レスポンス: `content[0].text` に本文テキスト。ここから共通の JSON 抽出でパースする。

送信データは既存ポリシー通り「件名・送信者・本文冒頭300文字＋案件リスト＋（許可された場合の）案件ディレクトリコンテキスト」に限定する（agent.md / `2026-07-09-project-directory-context-design.md`）。プロンプト構築 (`classifier::prompt`) はプロバイダ非依存なので流用する。

### 5.1 モデル選択の根拠

Pigeonの分類は「小さなJSON1個を返す分類・抽出タスク」であり、メール1通ごとに1回叩く高ボリューム処理（初回同期は最大 `initial_sync_limit`=5000 通）。深い推論は不要でコストが効くため、デフォルトは最安の **Haiku 4.5**（$1/$5 per MTok）とする。精度を上げたいユーザーは自由入力欄で `claude-sonnet-5`（$3/$15、導入価格 $2/$10）等に変更できる。Opus 4.8 / Fable 5 はこのタスクにはオーバースペックのため推奨しない。

（参考: 現行モデル `claude-fable-5` / `claude-opus-4-8` / `claude-sonnet-5` / `claude-haiku-4-5`）

## 6. Tauri commands（新規）

`commands/settings_commands.rs`（新規ファイル）に配置し、`lib.rs` の `invoke_handler` に登録する。

| command | 引数 | 戻り値 | 説明 |
|---|---|---|---|
| `get_llm_settings` | なし | `LlmSettings` | プロバイダ・各モデル・「APIキー登録済みか」のbool。**キー本体は返さない** |
| `set_llm_settings` | `provider, ollama_endpoint, ollama_model, claude_model, claude_api_key` | `Result<(), String>` | 設定を保存。`claude_api_key` が空文字なら既存キーを変更しない（未入力扱い） |
| `test_llm_connection` | `provider, ollama_endpoint, ollama_model, claude_model, claude_api_key` | `Result<(), String>` | **画面の現在設定**（未保存でも可）でファクトリ構築 → `health_check()` を呼び成否を返す。保存済み設定には依存しない。`claude_api_key` が空/None なら SecureStore の保存済みキーにフォールバック |

`LlmSettings`（`models` に定義、UIへ返す型）:

```
provider: string
ollama_endpoint: string
ollama_model: string
claude_model: string
claude_api_key_set: bool   // キー本体ではなく登録有無のみ
```

### 6.1 health_check

- Ollama: 既存の `/api/tags` GET（実装済み）。
- Claude: 軽量な検証。`GET https://api.anthropic.com/v1/models`（`x-api-key` 認証）を叩き 2xx なら成功、401 等ならキー無効としてエラー。

## 7. UI（LlmSettingsDialog.tsx）

`CloudSettingsDialog.tsx` のモーダルパターン（`fixed inset-0 ... bg-black/40`）を踏襲する。配置は `src/components/sidebar/`。

### 7.1 構成要素

- **プロバイダ選択**（ラジオ）: Ollama / Claude / ChatGPT
  - ChatGPT は選択肢として表示するが disabled + 「未対応（今後対応予定）」表示。
- **Ollama 選択時**: エンドポイント入力・モデル入力（既存デフォルト値をプレースホルダ表示）。
- **Claude 選択時**:
  - APIキー入力（`type="password"`）。登録済みなら `••••••••` プレースホルダを表示し、空のまま保存すれば既存キー維持。
  - モデル自由入力欄（placeholder `claude-haiku-4-5`）。
  - **警告バナー**を表示（agent.md: クラウドAPI選択時はユーザーに警告）。文言例: 「クラウドLLMを使用します。件名・送信者・本文冒頭300文字と、許可した案件コンテキストが Anthropic に送信されます。」
- **接続テストボタン**: `test_llm_connection` を呼び、成功「接続成功」/失敗「接続失敗: <理由>」を表示。
- **保存ボタン / 閉じるボタン**。

### 7.2 状態と型

- 型定義 `src/types/settings.ts` に `LlmSettings` と `LlmProvider = "ollama" | "claude" | "openai"`。
- ダイアログを開くトリガー: サイドバーに設定（歯車）ボタンを追加し、押下で `LlmSettingsDialog` を開く。
- エラーは既存 `useErrorStore` 経由で表示。

## 8. エラー処理

`AppError` に追加:

- `MissingApiKey` — 「<プロバイダ> のAPIキーが未設定です。設定画面で登録してください。」
- `UnsupportedProvider(String)` — 「未対応のプロバイダです: <名前>」

いずれも Tauri command 境界で `String` に変換され、`useErrorStore` でUI表示される。フォールバックはしない（3.3参照）。

## 9. テスト方針（TDD: Red → Green → Refactor）

### Rust

- `classifier::parse`（共通化した抽出/パース）: 既存の Ollama 側テストを移設・拡充（surrounding text 付き、欠損フィールド、未知 action 等）。
- `settings::set`: UPSERT が新規挿入・上書き両方で機能すること。
- `classifier::factory::build_classifier`: provider ごとに
  - `ollama` → OllamaClassifier が構築される
  - `claude` + キーあり → ClaudeClassifier が構築される
  - `claude` + キーなし → `MissingApiKey`
  - `openai` → `UnsupportedProvider`
  （SecureStore はテスト用の一時 Stronghold もしくは trait 抽象で差し込む。HTTP 実通信はテストしない。）
- `ClaudeClassifier`: リクエストボディの組み立て（model/headers 相当のフィールド）と、レスポンス JSON からの `content[0].text` 取り出しをユニットで検証。

### React（Vitest + RTL）

- `LlmSettingsDialog`: 初期レンダリング、プロバイダ切替でフォームが切り替わること、Claude選択時に警告バナーが出ること、ChatGPTが disabled であること、接続テストボタンが `test_llm_connection` を呼ぶこと。`invoke` はモックする。

## 10. スコープ外（YAGNI）

- ChatGPT の実処理（今回は口だけ）。
- 用途別（分類とダイジェストで別プロバイダ）の切り替え。
- ストリーミング応答・prompt caching・Batches API 等の最適化。
- 別ウィンドウ（Tauri WebviewWindow）化。今回はモーダルダイアログ。

## 11. 影響ファイル一覧

**Rust（変更）**
- `classifier/mod.rs`（trait 統合、`parse` モジュール追加、`factory` モジュール追加）
- `classifier/ollama.rs`（抽出ロジックを共通側へ移動、trait 追従）
- `classifier/claude.rs`（新規）
- `classifier/factory.rs`（新規）
- `classifier/parse.rs`（新規）
- `db/settings.rs`（`set` 追加）
- `commands/settings_commands.rs`（新規）
- `commands/classify_commands.rs`, `commands/directory_commands.rs`, `lib.rs`（直接構築をファクトリ呼び出しに置換、command 登録）
- `error.rs`（`MissingApiKey`, `UnsupportedProvider` 追加）
- `models/`（`LlmSettings` 型）

**React（変更/新規）**
- `components/sidebar/LlmSettingsDialog.tsx`（新規）
- `types/settings.ts`（新規）
- サイドバーに設定ボタン追加（既存コンポーネント）
- `__tests__/LlmSettingsDialog.test.tsx`（新規）

## 12. Claude on Vertex AI（GCP Agent Platform）追加 — プロバイダ `claude_vertex`

- 追記日: 2026-07-10
- 既存の Anthropic 直 API プロバイダ `claude` は**変更せず残す**。GCP 経由は**別プロバイダ `claude_vertex`** として並置する。ユーザーは「Claude API（Anthropic 直）」と「Claude (GCP Vertex AI)」を独立に選べる。

### 12.1 動機

課金を GCP プロジェクトに集約し、Vertex AI 経由で Claude を使いたい。API キー方式ではなく **GCP サービスアカウント認証**を用いる。具体的なプロジェクト ID はリポジトリに記載せず、ユーザーのローカル設定（settings DB）にのみ保持する（§12.4 参照）。

### 12.2 API 仕様（2026-07-10 公式確認）

Anthropic 直 API との差分は次の3点のみ。ボディ・レスポンスの大半は Messages API と同一。

- **エンドポイント**（非ストリーミング `:rawPredict`）:
  ```
  https://{location}-aiplatform.googleapis.com/v1/projects/{project}/locations/{location}/publishers/anthropic/models/{model}:rawPredict
  ```
  ただし `location=global` のときだけホスト名にリージョン接頭辞が付かず `https://aiplatform.googleapis.com/...` になる（`endpoint_url` で分岐）。
- **リクエストボディ**: `model` は**ボディに含めない**（URL パスで指定）。代わりに `anthropic_version` を**ボディに**入れ、値は必ず `"vertex-2023-10-16"`。
  ```json
  {
    "anthropic_version": "vertex-2023-10-16",
    "system": "<システムプロンプト>",
    "messages": [{ "role": "user", "content": "<ユーザープロンプト>" }],
    "max_tokens": 1024
  }
  ```
- **認証**: `Authorization: Bearer <access_token>`（`x-api-key` ではない）。アクセストークンは SA JSON から OAuth2 スコープ `https://www.googleapis.com/auth/cloud-platform` で取得。必要 IAM ロールは `roles/aiplatform.user`（Vertex AI User）。
- **レスポンス**: 標準 Messages API と同形。`content[0].text` を共通の `classifier::parse` に渡す。
- **モデルID**: Vertex 上の ID は直 API と異なり、多くが `@YYYYMMDD` サフィックスを持つ。デフォルトは `claude-haiku-4-5@20251001`。サフィックスの有無はモデルごとに違うため、UI の自由入力で正確な文字列を渡す。

### 12.3 認証実装（Rust）

SA JSON → アクセストークン取得に `gcp_auth` クレート（`CustomServiceAccount::from_json`）を追加する。既存の `reqwest`（rustls-tls）と整合するよう rustls を使う。トークンはクレート側でキャッシュ・失効管理される。HTTP 実通信・トークン取得はユニットテストの対象外（ボディ組み立て・URL 生成・レスポンス抽出のみテスト）。

### 12.4 データ設計

秘密情報のみ Stronghold、それ以外は settings という既存の切り分けを踏襲する。

**settings テーブル（平文, key-value）**

| キー | デフォルト | 説明 |
|---|---|---|
| `vertex_project_id` | （空） | GCP プロジェクト ID。リポジトリには置かず、ユーザーのローカル DB にのみ保存 |
| `vertex_location` | `global` | リージョン。`global` はホスト名が特殊（`aiplatform.googleapis.com`、リージョン接頭辞なし）で料金も安いためデフォルト。他リージョンは `{location}-aiplatform.googleapis.com` |
| `vertex_model` | `claude-haiku-4-5@20251001` | Vertex 上のモデル ID |

**SecureStore（Stronghold, 暗号化）**

| キー | 説明 |
|---|---|
| `vertex_sa_json` | サービスアカウント JSON キー全体（暗号化保存） |

`llm_provider` は `ollama` / `claude` / `claude_vertex` / `openai` を取りうる。プロジェクト ID をリポジトリに書かない要件は、settings（ユーザーローカル DB、git 管理外）と SecureStore で完全に満たされる。ソース・設計書・コミットにプロジェクト ID を記載しない。

### 12.5 ファクトリ

`build_classifier_from_params` の match に `"claude_vertex"` を追加:

```
"claude_vertex" →
  sa_json = 明示引数 or SecureStore("vertex_sa_json")
    └─ None/空 → Err(MissingApiKey("claude_vertex"))
  project_id = settings("vertex_project_id")  └─ 空 → Err(MissingApiKey)
  location   = settings("vertex_location", "global")
  model      = settings("vertex_model", "claude-haiku-4-5@20251001")
  Ok(Box::new(ClaudeVertexClassifier::new(sa_json, project_id, location, model)?))
```

`claude`（Anthropic 直）と同じく**フォールバックしない**。SA JSON / project_id 未設定なら明示エラー。

### 12.6 health_check

`:rawPredict` に最小のダミーメッセージ（`max_tokens: 1`, `messages:[{role:user, content:"ping"}]`）を投げ、2xx なら成功。専用の軽量エンドポイントが無いため、実際に疎通・認証・権限をまとめて検証できるこの方式を採る。

### 12.7 Tauri commands 拡張

既存 `get/set/test_llm_settings` を拡張（コマンド追加はしない）:

- `LlmSettings` に `vertex_project_id: String`, `vertex_location: String`, `vertex_model: String`, `vertex_sa_json_set: bool`（本体は返さない）を追加。
- `set_llm_settings` / `test_llm_connection` に `vertex_project_id, vertex_location, vertex_model, vertex_sa_json: Option<String>` を追加。SA JSON が空文字なら既存を保持（`claude_api_key` と同じ挙動）。

### 12.8 UI

- ラジオに「Claude (GCP Vertex AI)」を追加。
- `claude_vertex` 選択時: SA JSON 貼り付け欄（`textarea`, 登録済みなら `••••` プレースホルダ）／プロジェクト ID／リージョン／モデルの入力欄／クラウド警告バナー（送信先が Google Cloud (Vertex AI) である旨）。
- 接続テストボタンは既存同様 `test_llm_connection` を現在の画面設定で呼ぶ。

### 12.9 セキュリティ

- SA JSON は Stronghold 暗号保存。settings（平文 DB）・リポジトリには置かない（agent.md セキュリティルール準拠）。
- クラウド送信の警告文言を Vertex 用に出す（送信先が Anthropic ではなく Google Cloud (Vertex AI) 上の Claude であることを明示）。送信データ範囲は既存ポリシー（件名・送信者・本文冒頭300文字＋許可された案件コンテキスト）と同一。

### 12.10 影響ファイル（追加分）

**Rust**
- `Cargo.toml`（`gcp_auth` 追加）
- `classifier/claude_vertex.rs`（新規）
- `classifier/mod.rs`（`claude_vertex` モジュール公開）
- `classifier/factory.rs`（`claude_vertex` 分岐、引数追加）
- `commands/settings_commands.rs`（vertex フィールド対応）
- `models/settings.rs`（`LlmSettings` に vertex フィールド追加）

**React**
- `types/settings.ts`（`LlmProvider` に `claude_vertex`、`LlmSettings` に vertex フィールド）
- `components/sidebar/LlmSettingsDialog.tsx`（Vertex フォーム追加）
- `__tests__/LlmSettingsDialog.test.tsx`（Vertex 選択のテスト）
