# 案件ノート（Project Notes） 設計書

- 作成日: 2026-07-19
- ステータス: レビュー中
- 関連: `docs/design/2026-04-12-pigeon-design.md`（全体設計）、`docs/design/2026-07-09-project-directory-context-design.md`（案件ディレクトリ連携・PIGEON-CONTEXT.md）、`docs/design/2026-07-18-hierarchical-projects-design.md`（案件階層化）、`docs/adr/0002-cloud-llm-data-boundary.md`（クラウド送信境界）

## 1. 背景と目的

現状、案件（project）に紐づく自由記述の情報は `PIGEON-CONTEXT.md` として実装済みだが、これは **案件にディレクトリを連携したときにのみ** 生成される（`src-tauri/src/project_context/`）。ディレクトリを連携しない案件には案件情報を書き留める場所がない。

実運用ではディレクトリを連携しない案件の方が多いと見込まれる。すべての案件が、連携の有無に関わらず「案件ノート」を持てるようにする。

加えて、正本を Markdown ファイルとして外部エディタで編集する現行方式は、エンジニアでない一般ユーザーには馴染みがない。**アプリ内の WYSIWYG エディタ**で編集できるようにする。

## 2. スコープ

### やること
- 案件ノートの正本を **SQLite に一本化**（新テーブル `project_notes`）。ディレクトリ連携の有無に関わらず全案件がノートを持てる
- **アプリ内 WYSIWYG エディタ**（既存メール作成用 TipTap を流用）で編集。見出し・太字・斜体・箇条書き・番号リスト・リンク・**表**をサポート（画像は不要）
- ノートは2区画: **ユーザー手書きノート** と **AI要約（メール群から生成）**。UI 上はタブで分離
- **AI要約もユーザーが編集可能**。AI生成は「初回の下書き」であり、以後ユーザーが自由に修正できる
- AI再生成は上書き前に確認ダイアログ、旧バージョンを**履歴として保持**（戻せる）
- 既存の `PIGEON-CONTEXT.md`（ディレクトリ連携時）との**双方向同期**。正本は常に DB
- 既存の分類プロンプト注入（`cached_context`）を無改修で維持

### やらないこと（YAGNI）
- メール追加時のAI要約自動生成（まず手動ボタンのみ。将来別PR）
- 画像の埋め込み（表まで。画像は対象外）
- AI要約履歴の無限保持（直近N件のみ、古いものは削除）
- ノートの全文検索対象化（将来拡張。本設計では検索対象にしない）

## 3. 確定要件

1. **保存先**: 案件ノートの正本は SQLite（新テーブル `project_notes`）。`project_contexts` とはテーブルを分ける（責務分離: `project_contexts` はディレクトリ由来のキャッシュ+メタ、`project_notes` はユーザー編集ノート）
2. **保存形式**: Markdown（GFM、表を含む。画像なし）。WYSIWYG編集は TipTap、保存時に Markdown へ変換、読込時に Markdown → TipTap
3. **2区画**: `user_md`（ユーザー手書き）と `ai_md`（メール群からAI生成、ただしユーザー編集可）を別カラムで保持。UI はタブで分離
4. **AI生成のLLM境界**: 既存 classifier と同一ポリシー（件名・送信者・本文冒頭1000文字）。デフォルト Ollama、クラウド選択時は既存の警告フローに乗る（ADR-0002）
5. **AI再生成**: 手動ボタン。`ai_md` に手修正がある場合（`ai_edited=true`）は確認ダイアログを経てから上書き。旧 `ai_md` は履歴へ退避
6. **ディレクトリ連携時の同期**: 正本は常に DB。保存時に `user_md`+`ai_md` を `upsert_auto_section` で `PIGEON-CONTEXT.md` に合成しファイルへ書き出す（ミラー）。再スキャンでファイルの外部編集を検知したら `split_at_marker` で2カラムへ戻し DB を更新（自己修復）
7. **分類パイプライン無改修**: `project_contexts.cached_context`（800字）は `project_notes` 保存時に `build_cached_context` で再生成して従来通り反映

## 4. データモデル

migration は次の空き番号を消費する（現状の最新は v20 のため v21 想定。**実装時に最新の migration を確認**すること — 並行作業で番号が進んでいる可能性がある）。

```sql
-- 案件ノート（正本）。ディレクトリ連携の有無に関わらず全案件が持てる。
CREATE TABLE project_notes (
    project_id      TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    user_md         TEXT NOT NULL DEFAULT '',        -- 「ノート」タブ: ユーザー編集（Markdown/GFM）
    ai_md           TEXT,                            -- 「AI要約」タブ: AI下書き + ユーザー編集可
    ai_edited       BOOLEAN NOT NULL DEFAULT FALSE,  -- ユーザーが ai_md を手修正したか（再生成時の確認判定）
    ai_generated_at DATETIME,                        -- 最後にAI生成した時刻
    updated_at      DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- AI要約の再生成履歴（戻せるように）。直近N件のみ保持。
CREATE TABLE project_note_ai_history (
    id            TEXT PRIMARY KEY,
    project_id    TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    ai_md         TEXT NOT NULL,
    replaced_at   DATETIME DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_project_note_ai_history_project
    ON project_note_ai_history(project_id);
```

- 履歴の保持数は直近10件を上限とし、退避時に古いものを削除する（無限に溜めない）。
- `project_notes` の行は遅延生成でよい（初回保存 or 初回AI生成時に INSERT）。存在しない場合は空ノートとして扱う。

## 5. バックエンド（Rust）

### モジュール構成
- `src-tauri/src/db/project_notes.rs`（新規）: `project_notes` / `project_note_ai_history` の CRUD
  - `get_note(project_id) -> Option<ProjectNote>`
  - `upsert_user_md(project_id, user_md)`
  - `upsert_ai_md(project_id, ai_md, mark_edited: bool)` — ユーザー手編集時は `ai_edited=true`
  - `replace_ai_md_with_history(project_id, new_ai_md)` — 旧 `ai_md` を履歴へ退避してから上書き、`ai_edited=false`、`ai_generated_at` 更新、履歴を10件に剪定
  - `list_ai_history(project_id)` / `restore_ai_from_history(history_id)`
- `src-tauri/src/models/project_note.rs`（新規）: `ProjectNote` 型
- マーカー処理（`split_at_marker` / `upsert_auto_section` / `build_cached_context`）は現状 `project_context/context_file.rs` にある純粋関数を**共有**する。`project_notes` と `project_context` の双方から使うため、必要なら `context_file.rs` を参照する形で再利用（移動は最小限に留め、責務が曖昧になるなら共有ヘルパへ切り出す）

### AI要約生成
- `src-tauri/src/project_context/` に既存の `digest.rs`（案件ダイジェスト生成）があるが、これはディレクトリのファイル一覧が入力。**メール版**の生成ロジックを新設する（`project_notes` 側 or 新モジュール `project_note_digest`）:
  - 入力: 案件に属するメール群（`mail_project_assignments` 経由）。各メールから**件名・送信者・本文冒頭1000文字**を集約（classifier と同一境界）
  - 件数上限: 直近50件でサンプリング。超過分は切り捨て、切り捨てた件数を `log` に残す（サイレント切り捨て禁止）
  - プロンプト: 既存 `digest.rs` を流用しつつメール向けに調整。「主なファイル」は落とし「主なやり取り/論点」等に置換。出力は Markdown（公演・会場・関係者・キーワード等の箇条書き）
  - LLM: 既存 `TextGenerator` 抽象。デフォルト Ollama、`cloud` フラグでクラウド（既存の警告フロー）
  - メール0件: 生成せず「対象メールがありません」を返す

### ディレクトリ連携との同期
- `project_notes` 保存時、当該案件がディレクトリ連携済みなら `user_md`+`ai_md` を `upsert_auto_section` で `PIGEON-CONTEXT.md` に書き出す（DB→ファイルのミラー）
- 既存 `rescan_project`（`project_context/mod.rs`）の自己修復ロジックを **DB正本前提** に付け替える: ファイルの外部編集を検知したら `split_at_marker` で `user_md`/`ai_md` に分解して `project_notes` を更新する（現状は `project_contexts.cached_context` のみ更新）
- `project_contexts.cached_context` は `project_notes` 保存時に `build_cached_context(user_md + ai_md)` で再生成して反映。分類パイプラインは無改修

### Tauri コマンド（`src-tauri/src/commands/`）
- `get_project_note(project_id) -> { user_md, ai_md, ai_edited, ai_generated_at }`
- `save_project_note_user(project_id, user_md)`
- `save_project_note_ai(project_id, ai_md)` — 手編集保存（`ai_edited=true`）
- `generate_project_note_ai(project_id, cloud) -> { ai_md, ai_generated_at }` — 履歴退避のうえ再生成
- `list_project_note_ai_history(project_id)` / `restore_project_note_ai(history_id)`
- すべて `Result<T, String>`。`unwrap`/`expect` 不使用、`thiserror` の `AppError` を使用

## 6. フロントエンド（React）

### コンポーネント
- `src/components/project-note/`（新規）
  - `ProjectNotePanel.tsx`: 案件選択時に表示。「ノート」/「AI要約」タブを持つ
  - `ProjectNoteEditor.tsx`: TipTap ラッパ（見出し・太字・斜体・箇条書き・番号リスト・リンク・表）。画像拡張は無効
  - AI要約タブ: 生成/再生成ボタン、確認ダイアログ（手修正あり時）、履歴表示・復元
- **配置**: 中央ペイン上部に案件ヘッダとして折りたたみ表示、クリックで編集展開（既存3ペインレイアウトを崩さない）

### Markdown ⇔ TipTap 変換
- 既存メール作成側の変換資産があれば流用。なければ `tiptap-markdown` 等の変換層を1つ設ける（GFM 表対応）
- `any` 不使用。invoke レスポンスは `src/types/` に型定義

### 状態管理
- Zustand ストア（`src/stores/`）にノート状態を追加。保存はデバウンス自動保存（メール下書きと同じ挙動）+ 明示保存

## 7. エラーハンドリング

- LLM生成失敗: `AppError` 経由で UI へ返す。失敗しても既存 `ai_md` は保持（消さない）
- メール0件で生成: 「対象メールがありません」を返し空生成しない
- ディレクトリへのファイル書き出し失敗: DB正本は保存済みとして扱い、ファイルミラーの失敗は警告に留める（DB更新はロールバックしない）
- 履歴復元: 復元対象が存在しない場合はエラーを返す

## 8. テスト（TDD）

### Rust
- `project_notes` CRUD（遅延生成含む）
- 履歴退避＆復元、10件剪定
- `ai_edited` フラグ遷移（AI生成で false、手編集で true）
- `user_md`+`ai_md` ⇔ `PIGEON-CONTEXT.md` の合成/分解ラウンドトリップ
- `cached_context` 再生成が従来と一致
- メール集約入力ビルダー（境界1000字・件数上限50・切り捨てログ）
- AI生成は モック `TextGenerator` で検証（実LLM不使用）

### React
- ノートタブ/AI要約タブの表示切替
- TipTap ↔ Markdown 変換（表を含むラウンドトリップ）
- 保存デバウンス
- 再生成の確認ダイアログ（手修正あり/なしの分岐）
- 履歴復元UI

## 9. セキュリティ

- AI要約生成のLLM送信は classifier と同一境界（件名・送信者・本文冒頭1000文字）。ADR-0002 のクラウド送信境界を越えない
- クラウドLLM選択時は既存の警告フローを踏襲
- ノート本文は案件所属メール由来のユーザーデータ。DB（SQLite）に保存され、ファイル由来データのクラウド送信ポリシー（`PIGEON-CONTEXT.md` 側の `allow_cloud_context`）とは別管理

## 10. 移行・互換

- 既存の `PIGEON-CONTEXT.md`（ディレクトリ連携済み案件）は、初回同期時に `split_at_marker` で `project_notes` へ取り込む（ファイル→DB の一方向初期化）。以後は DB 正本
- 既存の `project_contexts` テーブルはそのまま残す（`cached_context` の生成元が `project_notes` に変わるだけ）
