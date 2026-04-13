# Phase 2: AI分類 設計書

## 概要

メールをAIによって案件（Project）に自動分類する機能を実装する。ユーザーが手動で案件を作成した上で、未分類メールに対して分類ボタンを押すとLLMが各メールを適切な案件に振り分ける。

### 基本方針

| 項目 | Phase 2 での対応 |
|------|-----------------|
| LLMプロバイダ | Ollama（外部プロセス、HTTP API） |
| 対応モデル | Llama 3系列（3.1/3.3）、Qwen3.5 4B/9B（設定で切り替え可能） |
| 分類タイミング | 手動トリガー（UIボタン） |
| 通信方式 | REST API、一括JSON応答（非ストリーミング） |
| 分類単位 | 1通ずつ個別分類 |

### 後続Phaseへの申し送り

| 項目 | 対応Phase | 備考 |
|------|----------|------|
| バッチ分類（N通まとめて） | Phase 5 | 初回同期時の大量メール分類で必要。コンテキスト超過に注意 |
| バックグラウンド自動分類 | Phase 3+ | 同期後に自動で分類キューを処理する |
| ストリーミング応答 | Phase 5 | Claude API対応時に検討 |
| `llama.cpp` 組み込み | Phase 5-6 | 一般ユーザー向けリリースにはOllamaの別途インストールが不要な形が必須 |
| Claude API対応 | Phase 5 | クラウドAPI選択時はユーザーに警告表示 |

### LLMプロバイダ戦略（リリース時）

一般ユーザー向けリリースでは、ユーザーがOllamaを別途インストールしている前提にはできない。
最終的なプロバイダ構成は以下を想定する：

| プロバイダ | 方式 | 対象ユーザー |
|-----------|------|------------|
| `llama.cpp` 組み込み | アプリ内でローカル推論（モデルは初回DL） | デフォルト。プライバシー重視 |
| Ollama | 外部プロセスへHTTP接続 | 開発者・パワーユーザー |
| Claude API | クラウドAPI | 速度・精度重視のユーザー |

`LlmClassifier` trait で抽象化しているため、後から `LlamaCppClassifier` / `ClaudeClassifier` を追加するだけで対応可能。

---

## 1. データモデル

### データ階層

```
Account > Project > Mail（mail_project_assignments経由）> Thread（スレッド構築）
```

案件はアカウントに紐づく。異なるアカウントのメールは別の案件空間で管理される。

### V3 マイグレーション

既存の `schema_version = 2` から以下のテーブルを追加する。

**重要**: マイグレーション実行前に `PRAGMA foreign_keys = ON` を有効化する（V1/V2 では未設定だったため、既存の `REFERENCES` 制約も実質無効だった）。

```sql
PRAGMA foreign_keys = ON;

-- 案件（アカウントに紐づく）
CREATE TABLE projects (
    id          TEXT PRIMARY KEY,
    account_id  TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    description TEXT,
    color       TEXT,
    is_archived BOOLEAN DEFAULT FALSE,
    created_at  DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at  DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_projects_account ON projects(account_id);

-- メール→案件の紐付け
CREATE TABLE mail_project_assignments (
    mail_id        TEXT PRIMARY KEY REFERENCES mails(id) ON DELETE CASCADE,
    project_id     TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    assigned_by    TEXT NOT NULL CHECK(assigned_by IN ('ai', 'user')),
    confidence     REAL,
    corrected_from TEXT,
    created_at     DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_mpa_project ON mail_project_assignments(project_id);

-- アカウント分離の不変条件: mail と project が同一アカウントであることを強制
CREATE TRIGGER trg_mpa_account_check
BEFORE INSERT ON mail_project_assignments
BEGIN
    SELECT CASE
        WHEN (SELECT account_id FROM mails WHERE id = NEW.mail_id)
          != (SELECT account_id FROM projects WHERE id = NEW.project_id)
        THEN RAISE(ABORT, 'mail and project must belong to the same account')
    END;
END;

CREATE TRIGGER trg_mpa_account_check_update
BEFORE UPDATE OF project_id ON mail_project_assignments
BEGIN
    SELECT CASE
        WHEN (SELECT account_id FROM mails WHERE id = NEW.mail_id)
          != (SELECT account_id FROM projects WHERE id = NEW.project_id)
        THEN RAISE(ABORT, 'mail and project must belong to the same account')
    END;
END;

-- 手動修正履歴（Phase 3 で本格利用。Phase 2 ではテーブル作成のみ）
-- NOTE: Phase 3 で書き込みを開始する際、mail_project_assignments と同様の
-- アカウント整合性トリガー（mail.account_id == project.account_id）を追加すること
CREATE TABLE correction_log (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    mail_id        TEXT NOT NULL REFERENCES mails(id) ON DELETE CASCADE,
    from_project   TEXT REFERENCES projects(id) ON DELETE SET NULL,
    to_project     TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    corrected_at   DATETIME DEFAULT CURRENT_TIMESTAMP
);
```

### PRAGMA foreign_keys の適用方針

`PRAGMA foreign_keys = ON` はコネクションごとに設定が必要（SQLiteの仕様）。
`lib.rs` の `Connection::open` 直後に実行し、アプリ全体で外部キー制約を有効にする。

### インメモリ状態（Tauri State）

LLMが「新規案件を提案」した場合のユーザー承認待ち状態。DBには永続化しない。
`classify_unassigned` で複数の `create` 提案が出る場合があるため、`HashMap<mail_id, PendingClassification>` で管理する。

```rust
struct PendingClassification {
    result: ClassifyResult,
    created_at: DateTime<Utc>,
}

// Tauri State として管理
struct PendingClassifications(Mutex<HashMap<String, PendingClassification>>);
```

`approve_new_project` / `reject_classification` 実行時に該当エントリを削除する。

---

## 2. Rustモジュール構成

### 新規モジュール

```
src-tauri/src/
├── classifier/              # LLM分類レイヤー
│   ├── mod.rs               # LlmClassifier trait 定義
│   ├── ollama.rs            # OllamaClassifier 実装
│   └── prompt.rs            # プロンプト構築ロジック
├── models/
│   ├── project.rs           # Project, ProjectSummary 構造体
│   └── classifier.rs        # ClassifyResult, MailSummary, CorrectionEntry 等
├── db/
│   ├── projects.rs          # projects CRUD
│   └── assignments.rs       # mail_project_assignments CRUD
└── commands/
    ├── project_commands.rs  # 案件CRUD コマンド
    └── classify_commands.rs # 分類トリガー・結果承認コマンド
```

### LlmClassifier trait

```rust
#[async_trait]
pub trait LlmClassifier: Send + Sync {
    async fn classify(
        &self,
        mail: &MailSummary,
        projects: &[ProjectSummary],
        corrections: &[CorrectionEntry],
    ) -> Result<ClassifyResult, ClassifyError>;
}
```

`classify` の戻り値 `ClassifyResult` には `mail_id` を含めない。呼び出し元（`classify_commands.rs`）が `MailSummary` と `ClassifyResult` を紐付けて `ClassifyResponse`（mail_id + ClassifyResult）を構築する。

Phase 2 では `corrections` は空配列を渡す。Phase 3 で correction_log からデータを取得して渡す。

### OllamaClassifier

```rust
pub struct OllamaClassifier {
    endpoint: String,  // デフォルト: http://localhost:11434
    model: String,     // デフォルト: llama3.1:8b
    client: reqwest::Client,
}
```

- `POST /api/chat` に `{ "model": "...", "messages": [...], "stream": false }` を送信
- レスポンスの `message.content` からJSONをパースして `ClassifyResult` に変換
- 接続前に `GET /api/tags` でヘルスチェック
- 1通あたりのタイムアウト: 30秒（`reqwest::Client` の timeout で設定）

### 型定義

```rust
// LLMに送るメール要約（mail_id は含めない。LLMに不要な情報）
pub struct MailSummary {
    pub subject: String,
    pub from_addr: String,
    pub date: String,
    pub body_preview: String,  // 本文冒頭300文字
}

// LLMに送る案件要約
pub struct ProjectSummary {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub recent_subjects: Vec<String>,  // 直近3件のメール件名
}

// recent_subjects の取得:
// SELECT m.subject FROM mails m
//   JOIN mail_project_assignments mpa ON m.id = mpa.mail_id
//   WHERE mpa.project_id = ?1
//   ORDER BY m.date DESC LIMIT 3

// 手動修正履歴（LLMプロンプトに含める）
pub struct CorrectionEntry {
    pub mail_subject: String,
    pub from_project: Option<String>,
    pub to_project: String,
}

// LLMの分類結果（mail_id を含めない）
pub enum ClassifyAction {
    Assign { project_id: String },
    Create { project_name: String, description: String },
    Unclassified,
}

pub struct ClassifyResult {
    pub action: ClassifyAction,
    pub confidence: f64,
    pub reason: String,
}

// コマンドの戻り値（呼び出し元で mail_id を付与）
pub struct ClassifyResponse {
    pub mail_id: String,
    pub result: ClassifyResult,
}
```

---

## 3. Tauri Commands

### 案件管理

| コマンド | 引数 | 戻り値 | 説明 |
|---------|------|--------|------|
| `create_project` | account_id, name, description?, color? | Project | 案件を手動作成 |
| `get_projects` | account_id | Project[] | アカウントの案件一覧取得 |
| `update_project` | id, name?, description?, color? | Project | 案件を更新 |
| `archive_project` | id | — | 案件を論理削除（is_archived = true） |
| `delete_project` | id | — | 案件を物理削除（CASCADE で assignments, correction_log も削除） |

### 分類

| コマンド | 引数 | 戻り値 | 説明 |
|---------|------|--------|------|
| `classify_mail` | mail_id | ClassifyResponse | 1通を分類（手動トリガー） |
| `classify_unassigned` | account_id | — | 未分類メール全件を分類。進捗は Tauri events で通知 |
| `cancel_classification` | — | — | 実行中の分類を中止 |
| `approve_classification` | mail_id, project_id | — | 分類結果を承認または修正。project_id がAI提案と異なる場合は割り当て先を変更 |
| `approve_new_project` | mail_id, project_name, description? | Project | 新規案件提案を承認。案件を作成し、メールを割り当て |
| `reject_classification` | mail_id | — | 分類結果を破棄（assignments から削除し未分類に戻す） |

#### `approve_new_project` の処理フロー

単一トランザクションで実行し、途中失敗時はロールバックする。

1. BEGIN TRANSACTION
2. `projects` テーブルに新規案件を INSERT（`account_id` はメールのアカウントから取得）
3. `mail_project_assignments` にメールと新規案件の紐付けを INSERT
4. COMMIT
5. 新規作成した `Project` を返却

いずれかのステップで失敗した場合は ROLLBACK し、エラーを返す。

#### `approve_classification` の処理フロー

`approve_classification(mail_id, project_id)` は「承認」と「修正」の両方を兼ねる：

- **承認**: AI が提案した `project_id` と同じ値を渡す → `assigned_by` を `'user'` に UPDATE
- **修正**: 別の `project_id` を渡す → `project_id` と `assigned_by` を UPDATE、`corrected_from` に旧 project_id を記録

```sql
UPDATE mail_project_assignments
SET project_id = ?1,
    assigned_by = 'user',
    corrected_from = CASE WHEN project_id != ?1 THEN project_id ELSE corrected_from END
WHERE mail_id = ?2
```

#### `classify_unassigned` の進捗通知

Tauri events でフロントエンドに進捗を通知する。ポーリングは行わない。

```rust
// 進捗イベント
handle.emit("classify-progress", ClassifyProgress {
    current: 3,
    total: 15,
    mail_id: "...",
    result: ClassifyResponse { ... },
})?;

// 完了イベント
handle.emit("classify-complete", ClassifySummary {
    total: 15,
    assigned: 10,
    needs_review: 3,
    unclassified: 2,
})?;
```

ユーザーが `cancel_classification` を呼ぶと、内部の `AtomicBool` フラグを立ててループを中断する。

### メール取得

| コマンド | 引数 | 戻り値 | 説明 |
|---------|------|--------|------|
| `get_unclassified_mails` | account_id | Mail[] | 未分類メール一覧 |
| `get_mails_by_project` | project_id | Mail[] | 案件に紐づくメール一覧 |

---

## 4. LLMプロンプト設計

### システムプロンプト

```
You are an email classifier. Given an email and a list of existing projects,
determine which project the email belongs to.

Respond with ONLY a JSON object in one of these formats:

1. Assign to existing project:
{"action": "assign", "project_id": "<id>", "confidence": 0.85, "reason": "..."}

2. Propose new project:
{"action": "create", "project_name": "<name>", "description": "<desc>", "confidence": 0.78, "reason": "..."}

3. Cannot classify:
{"action": "unclassified", "confidence": 0.30, "reason": "..."}

Rules:
- confidence is a float between 0.0 and 1.0
- reason is a brief explanation in Japanese
- When no existing project matches well, use "create" to propose a new one
- Use "unclassified" only when the email content is too ambiguous to classify
```

### ユーザープロンプト（メールごとに動的構築）

以下は擬似コード表記。実装では Rust の `format!` で構築する。

```
## Existing Projects
(for each project)
- ID: {id} | Name: {name} | Description: {description} | Recent subjects: {recent_subjects}
(end)

## Email to Classify
- Subject: {subject}
- From: {from_addr}
- Date: {date}
- Body (first 300 chars): {body_preview}

## Recent Corrections (for reference)
(for each correction)
- Email "{mail_subject}" was moved from "{from_project}" to "{to_project}"
(end)
```

### 設計ポイント

- プロンプトは英語（LLMの指示理解力が安定する）
- `reason` は日本語指定（UIに表示するため）
- 本文は300文字に切り詰め（設計書のセキュリティ要件に準拠）
- `recent_subjects` は案件内の直近メール件名3件（案件の文脈をLLMに伝える）
- テンプレートエンジンは使わず、Rustの `format!` で構築
- JSONパース失敗時は `ClassifyAction::Unclassified`（confidence: 0.0）にフォールバック

---

## 5. 分類の状態遷移

### 確信度による初期状態

| 確信度 | action | 初期状態 | DB操作 |
|-------|--------|---------|--------|
| >= 0.7 | assign | `applied` — 自動で割り当て | `mail_project_assignments` に INSERT（`assigned_by = 'ai'`） |
| 0.4 〜 0.7 | assign | `applied` — 割り当てるが要確認 | 同上（⚠ バッジで視覚的に区別） |
| < 0.4 | assign / unclassified | `unclassified` — 未分類のまま | INSERT しない |
| any | create | `pending_proposal` — ユーザー承認待ち | INSERT しない。インメモリの `PendingClassification` に保持 |

### ユーザー操作による状態遷移

```
[applied (>= 0.7)]
  └─ approve_classification → confirmed（assigned_by を 'user' に UPDATE）
  └─ reject_classification → unclassified（assignments から DELETE）

[applied (0.4〜0.7)] ⚠マーク付き
  └─ approve_classification → confirmed（assigned_by を 'user' に UPDATE、⚠ を外す）
  └─ reject_classification → unclassified（assignments から DELETE）

[pending_proposal]
  └─ approve_new_project → 案件作成 + applied（assigned_by = 'user'）
  └─ reject_classification → unclassified（PendingClassification から削除）

[unclassified]
  └─ classify_mail → 再分類
```

すべての確信度で `approve_classification` が使用可能。>= 0.7 の自動割り当ても後からユーザーが確認・修正できる。

### 確認済み状態の永続化

確認済み（confirmed）かどうかは `assigned_by` カラムで判定する：

- `assigned_by = 'ai'` → AI分類のまま（未確認）。確信度 0.4〜0.7 なら ⚠ 表示
- `assigned_by = 'user'` → ユーザーが承認済み。⚠ を表示しない

`approve_classification` 実行時に `assigned_by` を `'ai'` → `'user'` に UPDATE する。
これにより再起動後も確認済み状態が維持される。新規カラムの追加は不要。

### 確信度閾値

閾値はコード内の定数として一箇所に集約する。

```rust
pub const CONFIDENCE_AUTO_ASSIGN: f64 = 0.7;
pub const CONFIDENCE_UNCERTAIN: f64 = 0.4;
```

将来的にモデルごとの確信度分布が安定したら、`settings` テーブルで調整可能にすることを検討する。

---

## 6. UI設計

### サイドバー変更

現在のアカウント一覧の下に、選択中アカウントの案件ツリーと未分類セクションを追加する。

```
│ ▼ Account A   │  ← 選択中
│   Account B   │
│ ────────────  │
│ ▶ 案件A (8)    │  ← Account A の案件
│ ▶ 案件B (3)    │
│ ────────────  │
│ ⚠ 未分類 (2)   │  ← Account A の未分類メール
│ ────────────  │
│ ＋ 案件を作成   │  ← 手動作成ボタン
```

### 新規コンポーネント

| コンポーネント | パス | 説明 |
|--------------|------|------|
| `ProjectTree` | `components/sidebar/ProjectTree.tsx` | 案件ツリー表示 |
| `ProjectForm` | `components/sidebar/ProjectForm.tsx` | 案件作成/編集フォーム |
| `UnclassifiedList` | `components/thread-list/UnclassifiedList.tsx` | 未分類メール一覧 |
| `ClassifyButton` | `components/thread-list/ClassifyButton.tsx` | 分類実行ボタン + プログレス + キャンセル |
| `ClassifyResultBadge` | `components/common/ClassifyResultBadge.tsx` | 確信度バッジ（色分け） |
| `NewProjectProposal` | `components/common/NewProjectProposal.tsx` | 新規案件提案ダイアログ |

### Zustand ストア

| ストア | 説明 |
|-------|------|
| `projectStore.ts` | 案件一覧、選択中の案件、CRUD操作 |
| `classifyStore.ts` | 分類状態（実行中/結果/ペンディング提案/キャンセル） |

### インタラクション

| 操作 | アクション |
|------|-----------|
| アカウント切替 | 案件ツリーが選択アカウントの案件に切り替わる |
| 案件クリック | 中央ペインにその案件のスレッド一覧を表示 |
| 「未分類」クリック | 中央ペインに未分類メール一覧を表示 |
| 「分類する」ボタン | 未分類メール全件をLLMで分類。プログレスバー + キャンセルボタン表示 |
| ⚠ バッジクリック | 「正しい / 修正する」の選択肢を表示 |
| 新規案件提案ダイアログ | 案件名を編集可能。「作成」で案件作成+メール割り当て |
| 「＋ 案件を作成」 | 案件名・説明・色を入力するフォーム |

---

## 7. 設定

### settings テーブル

Phase 2 では設定画面UIは作らず、`settings` テーブルを直接参照する。
キーが存在しない場合はコード内定数のデフォルト値を使う。

値はすべて文字列として保存し、型変換はアプリケーション側で行う。

| key | デフォルト値 | 説明 |
|-----|------------|------|
| `llm_provider` | `ollama` | LLMプロバイダ（Phase 2 では固定） |
| `ollama_endpoint` | `http://localhost:11434` | OllamaのベースURL |
| `ollama_model` | `llama3.1:8b` | 使用するモデル名 |

---

## 8. エラーハンドリング

### AppError 追加

```rust
#[error("Classifier error: {0}")]
Classifier(String),

#[error("Ollama connection failed: {0}")]
OllamaConnection(String),

#[error("Invalid LLM response: {0}")]
InvalidLlmResponse(String),

#[error("Project not found: {0}")]
ProjectNotFound(String),
```

### Ollama未起動時

分類実行前に `GET /api/tags` でヘルスチェックを行う。接続できない場合は
「Ollamaが起動していません。`ollama serve` を実行してください」
というエラーメッセージをUIに表示する。

### LLMレスポンス不正時

JSONパースに失敗した場合は `ClassifyAction::Unclassified`（confidence: 0.0）にフォールバックする。リトライは行わない。

### 分類タイムアウト

1通あたり30秒のタイムアウトを設定する（`reqwest::Client` の timeout）。タイムアウト時はその1通を `Unclassified` として扱い、次のメールに進む。
