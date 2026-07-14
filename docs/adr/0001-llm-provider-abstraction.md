# ADR 0001: LLMプロバイダ抽象化とフォールバック禁止方針

## ステータス

確定（2026-07-14）。

この決定はすでに実装済みである。中核は `src-tauri/src/classifier/` 配下にあり、trait 抽象は `classifier/mod.rs`、プロバイダ判定と構築の一本化は `classifier/factory.rs` に集約されている。本 ADR は複数の設計書に分散していた「LLM をどう抽象化し、どのプロバイダをどう切り替えるか」の決定を、実装を正として1本に集約したものである。

## コンテキスト（背景）

Pigeon は AI によってメールを案件ごとに自動グルーピングするデスクトップメールクライアントであり、その中核機能が LLM に依存する。LLM は単一の用途ではなく、少なくとも次の2つの用途で使われる。

- **分類**: メール1通を既存案件へ割り当てるか、新規案件を提案するか、未分類とするかを判定する。小さな JSON オブジェクトを1個返す抽出タスクであり、メール1通ごとに1回叩く高ボリューム処理である（初回同期は最大 `initial_sync_limit`=5000 通）。
- **テキスト生成**: 案件ディレクトリコンテキストのダイジェスト生成など、自由記述のテキストを生成する用途。

さらに、Pigeon はプライバシーを最優先とするため、LLM の実行先を次のように切り替えたいという要件がある。

- **ローカル（Ollama）**: データがネットワーク外に一切出ない。デフォルトかつセキュリティ最優先。
- **クラウド（Claude API / Claude on Vertex AI 等）**: 速度・精度を重視するユーザーが、明示的に選択した場合のみ使う。

この「複数用途 × 複数実行先」を素朴に実装すると、プロバイダ構築ロジックがアプリ全体に散らばる。実際、プロバイダ選択機能を導入する前は `OllamaClassifier::new(&endpoint, &model)` が3ファイル・4箇所（起動時スキャン・分類コマンド2箇所・再スキャン）で直接構築されており、プロバイダを増やすとこの分散が破綻することが見えていた。

加えて将来構想として、外部 LLM から Pigeon を操作させる MCP サーバー化や、アプリ内に常駐する AI エージェントが計画されている。これらはいずれも「エージェントの LLM 呼び出し」を必要とするが、そのために新しい LLM クライアントを作るのは重複であり、既存の抽象を再利用できる形にしておく必要がある。

以上より、LLM プロバイダの抽象化方針と、プロバイダ切り替え時の挙動（特にキー未設定時の扱い）を、アプリ全体で一貫した1つの決定として固める必要があった。

## 決定

### 1. 単一 trait でプロバイダを抽象化する

分類とテキスト生成を別々の抽象にせず、1つの trait object で両用途を賄う。`TextGenerator` を `LlmClassifier` のスーパートレイトとし、`Box<dyn LlmClassifier>` 1つで `generate_text`（テキスト生成）も `classify`（分類）も呼べるようにする。

```rust
#[async_trait]
pub trait TextGenerator: Send + Sync {
    async fn generate_text(&self, system_prompt: &str, user_prompt: &str)
        -> Result<String, AppError>;
}

#[async_trait]
pub trait LlmClassifier: TextGenerator + Send + Sync {
    async fn classify(/* mail, projects, corrections */) -> Result<ClassifyResult, AppError> { /* デフォルト実装 */ }
    async fn health_check(&self) -> Result<(), AppError>;
}
```

- `classify` は「プロンプト組み立て → `generate_text` 呼び出し → パース」という全プロバイダ共通の流れを **trait のデフォルト実装**として持つ。各プロバイダは `generate_text` と `health_check` だけを実装すればよい。
- プロンプト構築（`classifier::prompt`）と JSON 抽出・パース（`classifier::parse`）はプロバイダ非依存の共通モジュールに置き、全プロバイダから使う。パース失敗時は `ClassifyAction::Unclassified`（confidence: 0.0）にフォールバックする。

### 2. プロバイダ構築をファクトリ1本に集約する

プロバイダの判定と構築ロジックを `classifier::factory` に一元化する。分散した直接構築は全てファクトリ呼び出しへ置き換える。

- `build_classifier(conn, secure_store)` — 保存済み設定（settings テーブル＋ SecureStore）からプロバイダを判定して構築する。
- `build_classifier_from_params(params, secure_store)` — 画面上の（まだ保存していない）設定から構築する。接続テスト用。

現在サポートするプロバイダは以下（`llm_provider` 設定値 → 実体）。

| `llm_provider` | 実体 | 認証 | 既定モデル |
|---|---|---|---|
| `ollama`（既定） | `OllamaClassifier` | 不要（ローカル） | `llama3.1:8b` |
| `claude` | `ClaudeClassifier`（Anthropic 直 API） | API キー（`x-api-key`） | `claude-haiku-4-5` |
| `claude_vertex` | `ClaudeVertexClassifier`（Claude on Vertex AI） | GCP サービスアカウント JSON（`Bearer`） | `claude-haiku-4-5@20251001` |
| `gemini_vertex` | `GeminiVertexClassifier`（Gemini on Vertex AI） | GCP サービスアカウント JSON（`Bearer`） | `gemini-3.5-flash` |
| `openai` | 未実装 | — | UI の口だけ。選択すると `UnsupportedProvider` |

- プロバイダは**アプリ全体で単一**である。分類・ダイジェスト生成など、すべての用途で同じ選択プロバイダを使う（用途別に別プロバイダへ切り替える機能は持たない）。
- Claude 直（`claude`）と Claude on Vertex AI（`claude_vertex`）は課金経路と認証方式が異なるため、**別プロバイダとして並置**する。ユーザーは独立に選べる。
- Vertex 系（`claude_vertex` / `gemini_vertex`）は project_id / location / SA JSON を共有し、モデル ID のみ別に持つ。

### 3. 秘密情報は SecureStore、それ以外は settings テーブル

- API キー・SA JSON などの秘密情報は Stronghold ベースの `SecureStore`（暗号化）にのみ保存する。settings テーブル（平文 SQLite）やリポジトリには置かない。GCP プロジェクト ID も含め、ユーザーのローカル設定にのみ保持しリポジトリには記載しない。
- プロバイダ名・エンドポイント・モデル名などの非秘密情報は settings テーブルに置く。
- UI へ秘密情報の本体は返さない（「登録済みか」の bool のみ返す）。

### 4. キー未設定は明示エラー。サイレントフォールバックを禁止する

ファクトリはプロバイダごとに秘密情報の有無を検証し、**別プロバイダへ勝手に切り替えない**。

- `claude` 選択中に API キーが未設定・空なら、黙って Ollama に落とさず `AppError::MissingApiKey("claude")` を返す。
- `claude_vertex` / `gemini_vertex` は SA JSON または project_id が欠けていれば同様に `MissingApiKey` を返す。
- `openai` および未知の値は `AppError::UnsupportedProvider` を返す。
- 秘密情報の解決は「明示引数（非空）→ SecureStore の保存済み値」の順で行うが、これは**同一プロバイダ内**の解決であり、プロバイダをまたぐフォールバックではない。

これらのエラーは Tauri command 境界で `String` に変換され、フロントの `useErrorStore` 経由で UI に表示される。

### 5. 既定モデルは分類タスクに最適な最安モデルとする

- 分類は「小さな JSON を返す高ボリュームの抽出タスク」で深い推論を要さず、コストが効く。よって既定は各系列の最安モデル（Anthropic 直なら Haiku 4.5、$1/$5 per MTok）とする。
- 精度を上げたいユーザーはモデル名を自由入力で上書きできる（例: `claude-sonnet-5`）。Opus / Fable クラスはこのタスクにはオーバースペックのため既定にはしない。
- Vertex 上のモデル ID は `@YYYYMMDD` サフィックスの有無がモデルごとに異なるため、UI の自由入力で正確な文字列を渡す設計とする。

### 6. 将来のエージェント／MCP は同じ LLM 抽象を再利用する

MCP サーバーや常駐 AI エージェントを追加する際も、新しい LLM クライアントは作らず、この `TextGenerator` / `LlmClassifier` 抽象を再利用する。tool-calling 対応は既存抽象への薄い拡張で足りる。

## 理由

### なぜ単一 trait・ファクトリ一本化か

分類とテキスト生成で LLM クライアントを二重に持つと、プロバイダを1つ増やすたびに実装箇所が倍加する。単一 trait に統合し `classify` を共通デフォルト実装にすることで、新プロバイダは `generate_text` と `health_check` の2メソッドだけ書けばよくなる。構築ロジックのファクトリ集約は、agent.md の「不具合修正方針（負債を残しにくくする）」に沿い、その場しのぎの分岐追加ではなく構造の健全化を選ぶ判断である。

### なぜフォールバックしないか（最重要）

これは本 ADR の核心である。「クラウドプロバイダのキーが無ければローカルにフォールバックする」という一見親切な挙動を、意図的に禁止する。理由は次のとおり。

- **プライバシー境界を明示的に保つため**: Pigeon のデフォルトは Ollama（ローカル）であり、その最大の価値は「データがネットワーク外に一切出ない」ことにある。逆に、ユーザーがクラウドを選ぶという行為は「件名・送信者・本文冒頭のテキストを外部（Anthropic / Google Cloud）へ送る」ことへの明示的な同意である。この2つの状態は、ユーザーにとって意味がまったく異なる。
- **意図しない挙動の不一致を防ぐため**: フォールバックを許すと、「クラウドを使うつもりでキー入力を失敗したユーザー」が、気づかぬままローカル実行される（またはその逆に、設定不備で意図せずクラウドへ送られる）といった、ユーザーの意図と実挙動が食い違う状態を生む。特に後者はプライバシー事故につながる。明示エラーにすることで、ユーザーは自分が今どのプロバイダで動いているかを常に正しく把握できる。
- **デバッグ可能性のため**: サイレントフォールバックは「なぜ精度が変わったのか」「なぜ課金されない／されるのか」の原因を隠す。明示エラーは設定不備をその場で可視化する。

この方針は agent.md のセキュリティルール（デフォルトはローカル、クラウド選択時は警告表示、秘密情報は平文保存しない）と一貫している。

### なぜ Claude 直と Vertex を別プロバイダにするか

両者はエンドポイント・認証（`x-api-key` vs `Bearer`）・課金経路・モデル ID 体系が異なる。同一プロバイダ内のオプションとして畳むと分岐が複雑化し、ユーザーにとっても「どちらに課金されるか」が不透明になる。独立プロバイダとして並置することで、認証方式ごとに実装を素直に分離でき、ユーザーも送信先を明確に選べる。

### なぜ最安モデルを既定にするか

分類は高ボリュームかつ低難度のタスクであり、上位モデルの推論力はコストに見合わない。コスト最適な既定を置き、精度が必要なユーザーだけが自由入力で上げられる形が、大多数のユースケースにとって合理的である。

## 却下した代替案

- **プロバイダごとに別 trait を持つ（分類用 trait とテキスト生成用 trait を分離）**: ファクトリが用途ごとに別の trait object を返す必要があり、案件ダイジェスト生成（テキスト生成）と分類で別々の構築経路を維持することになる。プロバイダ追加コストが倍増するため却下。`TextGenerator` をスーパートレイトにまとめ、1つの trait object で両用途を賄う形を採用した。

- **キー未設定時にローカル（Ollama）へ自動フォールバックする**: 上記「なぜフォールバックしないか」のとおり、プライバシー境界を曖昧にし、意図しないクラウド送信やその逆を招く。明示エラー（`MissingApiKey`）を返す設計を採用した。

- **プロバイダ選択を持たず単一プロバイダに固定する**: ローカル優先というプライバシー方針と、速度・精度を求めるユーザーの両立ができない。単一だが**切り替え可能**な設計（アプリ全体で1プロバイダ、ただしユーザーが選べる）を採用した。

- **用途別プロバイダ（分類は Ollama、生成は Claude 等）を許す**: 設定の複雑さと、送信データの境界がユーザーにとって追いにくくなる問題があり、YAGNI として却下した。当面はアプリ全体で単一プロバイダとする。

- **秘密情報を settings テーブル（平文）に保存する**: セキュリティルール違反。SecureStore（Stronghold, 暗号化）にのみ保存する。

- **プロバイダ構築を各コマンドにインラインで残す**: プロバイダ追加のたびに分散した構築箇所を全て直す必要があり破綻する。ファクトリ集約を採用した。

## 影響

### この決定が縛る範囲

- LLM を呼ぶ全経路は `classifier::factory` を通じてプロバイダを構築する。コマンドやサービスがプロバイダを直接 `new` してはならない。
- 秘密情報は必ず SecureStore 経由。settings・ソース・コミット・設計書にキーやプロジェクト ID を書かない。
- プロバイダが未構築・未設定の場合は明示エラーを返す。どの層でもサイレントフォールバックを追加しない。
- 分類のプロンプト・パースはプロバイダ非依存の共通モジュールに置き、プロバイダ固有ロジックへ混ぜない。

### 新プロバイダを追加する手順

1. `classifier/<provider>.rs` を新規作成し、`generate_text` と `health_check` を実装する（`classify` は trait デフォルト実装を使う）。認証・エンドポイントの差分のみ書く。
2. `classifier/factory.rs` の `build_classifier_from_params` の `match` に分岐を追加する。秘密情報は `resolve_secret`（明示引数 → SecureStore）で解決し、欠けていれば `MissingApiKey` を返す。**フォールバックは足さない**。
3. `ClassifierParams` と settings のデフォルト定数に必要なフィールド・キーを追加する。
4. `settings_commands.rs` / `LlmSettings` 型を拡張し、UI にプロバイダ選択肢と（クラウドなら）送信警告バナーを追加する。
5. ファクトリのプロバイダ分岐テスト（キーあり→構築成功 / キーなし→ `MissingApiKey` / 未対応→ `UnsupportedProvider`）を追加する。HTTP 実通信はテストしない。

### 関連ファイル

- `src-tauri/src/classifier/mod.rs` — `TextGenerator` / `LlmClassifier` trait、`classify` デフォルト実装、共通 HTTP クライアント
- `src-tauri/src/classifier/factory.rs` — プロバイダ判定・構築の一本化、`ClassifierParams`、`resolve_secret`、フォールバック禁止
- `src-tauri/src/classifier/ollama.rs` / `claude.rs` / `claude_vertex.rs` / `gemini_vertex.rs` — 各プロバイダ実装
- `src-tauri/src/classifier/anthropic_common.rs` / `vertex_common.rs` — Messages API 形式・Vertex 共通ロジック
- `src-tauri/src/classifier/parse.rs` / `prompt.rs` — プロバイダ非依存のパース・プロンプト
- `src-tauri/src/db/settings.rs` — 非秘密設定の key-value ストア
- `src-tauri/src/secure_store.rs` — Stronghold ベースの秘密情報ストア
- `src-tauri/src/commands/settings_commands.rs` — `get/set/test_llm_settings`

## 参照

集約元の設計書（本 ADR 作成後、旧 spec 群は `docs/design/` および `docs/archive/specs/` へ移動予定のため、ここではパスを固定せず名称で示す）。

- 旧 pigeon-design（2026-04-12）— `LlmClassifier` trait の初出、Ollama デフォルト／Claude オプションの優先順位、LLM への送信データ範囲
- 旧 phase2-ai-classification-design（2026-04-13）— trait 定義、最終プロバイダ構成の想定、確信度ゲート、パース失敗フォールバック
- 旧 llm-provider-selection-design（2026-07-10）— ファクトリ集約、`TextGenerator` 統合、Claude / Vertex 追加、**フォールバックしない原則**、既定モデルの選択根拠。§12 で Claude on Vertex AI（`claude_vertex`）を追加
- 旧 ai-native-mcp-architecture-design（2026-07-14）— エージェント／MCP を driver として追加する際、既存 LLM 抽象（`TextGenerator` 等）を再利用する方針

### 版差についての注記

初期 spec（2026-04-13）は「最終的に `llama.cpp` 組み込みをデフォルトにする」構想を記していたが、実装は Ollama を既定とし `llama.cpp` 組み込みは未着手である。また実装には spec に明記のない `gemini_vertex`（Gemini on Vertex AI）プロバイダが追加されている。方針に矛盾がある場合は最新の実装（`classifier/factory.rs`）を正とする。
