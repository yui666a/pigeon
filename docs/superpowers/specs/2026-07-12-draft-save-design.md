# 下書き保存機能 設計書

## 概要

ComposeModal を閉じると入力が失われる問題に対応する。**v1 = ローカル下書きのみ**を実装する。
バックログ（`docs/BACKLOG.md` 項目4）の段階実装方針「ローカル下書き→IMAP Draftsフォルダ同期」の第一段階にあたる。

### スコープ

| 含む | 含まない（将来） |
|------|----------------|
| ローカルDBへの下書き保存（`drafts` テーブル） | IMAP Draftsフォルダへの同期（サーバー往復） |
| compose を閉じる際の自動保存 | 編集中のdebounce自動保存 |
| 下書き一覧の表示・選択での復元・削除 | 下書きの検索（FTS対象外） |
| 送信成功時の対応下書き削除 | 複数端末間の下書き共有（サーバー同期が前提） |

IMAP Drafts同期をスコープ外とする理由: サーバー往復（APPEND/削除/UID管理）はSentフォルダ同期（バックログ項目1の構造的課題）と設計上重なる。ローカル下書きは独立して価値があり、既存の `send_commands.rs` 同様 UIDPLUS 対応が固まってから同期を追加するのが妥当。

## アーキテクチャ

```
ComposeModal (React)
    │ closeCompose() 時、入力があれば自動保存
    ▼
draftStore.ts
    │ invoke("save_draft" / "get_drafts" / "delete_draft")
    ▼
commands/draft_commands.rs
    ▼
db/drafts.rs ── drafts テーブル (migration v12)
```

### モジュール構成

| ファイル | 責務 |
|---------|------|
| `src-tauri/src/db/migrations.rs` | `migrate_v12`: `drafts` テーブル新設 |
| `src-tauri/src/models/draft.rs` | `Draft` 構造体 |
| `src-tauri/src/db/drafts.rs` | `drafts` テーブルのCRUD |
| `src-tauri/src/commands/draft_commands.rs` | `save_draft` / `get_drafts` / `delete_draft` command |
| `src/stores/draftStore.ts` | 下書き一覧状態・CRUD呼び出し |
| `src/components/thread-list/DraftList.tsx` | 下書き一覧UI（サイドバーの「下書き」エントリから表示） |

## データ設計

### drafts テーブル（migration v12）

```sql
CREATE TABLE drafts (
    id          TEXT PRIMARY KEY,
    account_id  TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    to_addr     TEXT NOT NULL DEFAULT '',
    cc_addr     TEXT NOT NULL DEFAULT '',
    bcc_addr    TEXT NOT NULL DEFAULT '',
    subject     TEXT NOT NULL DEFAULT '',
    body_text   TEXT NOT NULL DEFAULT '',
    in_reply_to TEXT,
    created_at  DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at  DATETIME DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX idx_drafts_account ON drafts(account_id);
```

`to_addr`/`cc_addr`/`bcc_addr` は ComposeModal の入力欄と同じくカンマ区切り文字列で保持する（送信時にのみ配列へ分割する既存方針 `composePrefill.ts` に合わせる）。`in_reply_to` は返信元メールのローカルID（`SendMailRequest.reply_to_mail_id` と同じ意味）で、復元時にスレッディングを維持する。

### Draft（Rust / TypeScript 共有型）

```rust
pub struct Draft {
    pub id: String,
    pub account_id: String,
    pub to_addr: String,
    pub cc_addr: String,
    pub bcc_addr: String,
    pub subject: String,
    pub body_text: String,
    pub in_reply_to: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}
```

### コマンド

- `save_draft(req: SaveDraftRequest) -> Draft`: `req.id` が既存下書きのIDならUPDATE（updated_at更新）、無ければINSERT。新規作成時のIDはRust側でUUID生成し返す（フロントは次回保存時にそのIDを使う＝upsert）
- `get_drafts(account_id: String) -> Vec<Draft>`: アカウントの下書き一覧（updated_at降順）
- `delete_draft(id: String) -> ()`: 下書き削除。対象が存在しなくてもエラーにしない（送信成功時の削除で「既に無い」を許容するため）

## UI設計

### 自動保存（ComposeModal を閉じる時）

- Esc・×ボタン・「キャンセル」ボタンいずれも同じ `closeCompose()` を通る
- `closeCompose()` 内で to/cc/bcc/subject/body のいずれかに入力があれば `save_draft` を呼んでから閉じる。全て空なら保存しない（空の下書きが溜まるのを防ぐ）
- 送信中（`sending: true`）の場合は既存どおりクローズ操作自体を無効化（ComposeModal.tsx の既存ガード）
- 送信成功時は `send()` 内で対応する下書き（`draftId` があれば）を `delete_draft` してからクローズする

### 下書き一覧（DraftList）

- Sidebar に「下書き」エントリを追加（アカウント選択時のみ表示。既存の「✉ 新規作成」ボタン近辺）
- クリックで `viewMode` を `"drafts"` に切り替え、中央ペイン（`ThreadList` 等と同じ位置）に `DraftList` を表示
- 一覧は宛先・件名・更新日時のプレビュー表示。クリックで `openComposeFromDraft(draft)` により ComposeModal を復元（modeは常に `"new"` 相当の自由編集。返信元がある場合は `replyToMailId` を引き継ぐ）
- 各行に削除ボタン（確認なしで即削除。取り消し不要なほど低リスクな操作と判断）

### 状態管理

`draftStore.ts` に集約する。`composeStore.ts` は `draftId: string | null` フィールドを追加し、下書きから開いた場合／自動保存で新規作成した場合にセットする（既存 `replyToMailId` と同じ扱い）。`mailStore.ts` の変更は行わない。

## エラーハンドリング

- 自動保存の失敗はブロッキングにしない。保存に失敗してもモーダルは閉じる（下書き保存はベストエフォート。失敗を理由に閉じられなくなる方が使い勝手を損なう）。失敗時は `errorStore.addError` で通知
- `save_draft` / `get_drafts` / `delete_draft` は他コマンド同様 `Result<T, AppError>` → Tauri境界で文字列化

## テスト計画（TDD）

### Rust

- `migrations.rs`: v12 が `drafts` テーブルを作成すること、CASCADE削除（account削除時）
- `db/drafts.rs`: insert/update（upsert）、アカウント別一覧取得（updated_at降順）、削除（存在しなくてもエラーにしない）
- `commands/draft_commands.rs`: 新規保存でID採番、既存ID指定でupdated_at更新、空でも保存できること（バリデーションはUI側の責務とする）

### React

- `draftStore.ts`: save/get/delete の呼び出しとステート更新
- `composeStore.ts`: `closeCompose()` が入力ありで `save_draft` を呼ぶこと・入力なしで呼ばないこと、送信成功時に `delete_draft` を呼ぶこと
- `DraftList.tsx`: 一覧表示、クリックで復元、削除ボタンで `delete_draft` 呼び出し

## PR分割

本機能は単一PR `feat/draft-save` として実装する（バックエンド・フロントとも変更量が小さく、分割の意義が薄いため）。
