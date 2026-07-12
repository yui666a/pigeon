# 複数選択・一括操作 設計書

日付: 2026-07-13
ステータス: 実装対象
関連: `2026-07-12-mail-delete-archive-design.md`、`docs/BACKLOG.md` 項目3

## 目的

未分類一覧・案件別一覧のスレッド一覧で、複数のスレッド（＝メール群）を選択し、
一括削除・一括アーカイブ・一括で案件へ割り当てられるようにする。メール数が
増えると1件ずつの操作が現実的でなくなるため必須の操作。

## スコープ

- 対象: `ThreadList`（案件別一覧・INBOX一覧）と `UnclassifiedList`（未分類一覧）
- 選択粒度は既存の D&D（`useMailDrag`）と同じく **スレッド単位**。スレッド選択は
  そのスレッド内の全メールIDを選択対象にする（部分選択なし）
- v1 ではスレッド一覧の行のチェックボックスのみで選択する（Cmd/Ctrl+クリック等の
  複合操作はスコープ外。理由は下記「v1の制限」）

## 選択状態の管理（selectionStore）

新規ストア `src/stores/selectionStore.ts` に選択状態を集約する。`mailStore.ts` は
既存機能（同期・既読・単体削除等）で手一杯かつ他エージェントが並行編集中のため、
選択機能は独立ストアに分離する。

```ts
interface SelectionState {
  selectedThreadIds: Set<string>;
  toggleThread: (thread: Thread) => void;
  clear: () => void;
  isSelected: (threadId: string) => boolean;
  /** 選択中スレッドの全メールIDをフラット化して返す（一括操作の入力用） */
  selectedMailIds: (threads: Thread[]) => string[];
}
```

- `selectedThreadIds` は thread_id の集合のみを持つ（Mail 実体は持たない）。
  一括操作の実行時に、呼び出し側が保持している最新の `Thread[]`（`threads` /
  `unclassifiedThreads`）と突き合わせて mail_id を得る。ストア間の実体コピーを
  避け、選択解除漏れによる不整合（メール一覧更新後も selectionStore に古い
  Mail が残る等）を防ぐ
- 案件別一覧・未分類一覧・INBOX一覧は同時に1つしか表示されないため、選択状態は
  ビュー間で共有の単一ストアでよい（ビュー切替時に `clear()` する）

## 一括操作バー（BulkActionBar）

新規コンポーネント `src/components/thread-list/BulkActionBar.tsx`。
`selectedThreadIds` が空でない場合のみ一覧の上部に表示する。

- 表示内容: 選択件数、「削除」「アーカイブ」「案件へ移動」ボタン、「選択解除」
- 「案件へ移動」はプロジェクト一覧をドロップダウンで表示し選択すると即実行
  （既存の `useProjectStore().projects` を利用）
- 「削除」は既存の単体削除と同様 `window.confirm` で確認する
- 操作完了後（成功・部分失敗問わず）は選択解除し、一覧を再読み込みする
  （`fetchThreads` / `fetchUnclassified` を呼び出し元の画面に応じて実行）

## バックエンド（bulk_commands.rs）

新規ファイル `src-tauri/src/commands/bulk_commands.rs`。既存の `mail_commands.rs`
（他エージェントが編集中）は変更しない。

```rust
#[tauri::command]
pub async fn bulk_delete_mails(
    state: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    account_id: String,
    mail_ids: Vec<String>,
) -> Result<BulkResult, AppError>;

#[tauri::command]
pub async fn bulk_archive_mails(...) -> Result<BulkResult, AppError>;

#[tauri::command]
pub fn bulk_move_mails(
    state: State<'_, DbState>,
    mail_ids: Vec<String>,
    project_id: String,
) -> Result<BulkResult, AppError>;
```

```rust
#[derive(Serialize)]
pub struct BulkResult {
    pub succeeded: Vec<String>,
    /// (mail_id, エラーメッセージ) の組
    pub failed: Vec<(String, String)>,
}
```

### 部分失敗の扱い

1件の失敗で残りを止めない。各 mail_id を順に処理し、成功/失敗を `BulkResult` に
積み上げて返す（例外を投げるのは呼び出し全体が失敗するケースのみ、例:
`account_id` 自体が存在しない）。

- `bulk_delete_mails` / `bulk_archive_mails` は `mail_commands` にある
  `load_mail_context` / `plan_delete` / `plan_archive` 相当の判定ロジックを
  必要とするが、`mail_commands.rs` を触れないため、削除・アーカイブの
  IMAP 操作そのものは `mail_sync::imap_client` の関数を直接呼び出す形で
  `bulk_commands.rs` 内に薄く再実装する（1メールごとに `resolve_imap_credentials`
  → IMAP接続 → 操作、を繰り返すのは非効率だが、v1 は正しさ優先でこの形にする。
  接続の使い回しは将来の最適化）
- `bulk_move_mails` は `db::assignments::move_mail_to_project` をそのまま
  ループで呼ぶ（IMAP通信が無くDB操作のみのため単純）

### フロント側の呼び出し

`selectionStore` から得た mail_id 配列を渡す。`BulkResult` を受けて
`成功 N 件 / 失敗 M 件` をトースト表示する（`useErrorStore`）。失敗が
1件でもあれば内容を `console.error` に出す（個別メールIDまでは通常運用では
UIに出さない。v1の割り切り）。

## DBマイグレーション

不要。既存テーブル・カラムのみで完結する。

## v1の制限・スコープ外

- Cmd/Ctrl+クリックでの複合選択、Shift+クリックでの範囲選択は行わない。
  チェックボックスのみ（実装・テストの単純さを優先）
- 一括操作の進捗表示（N/M件処理中）は無し。件数が少ない個人利用を想定し、
  完了後の結果表示のみ
- 一括削除・一括アーカイブは IMAP 接続をメールごとに張り直す（上記）。
  数百件規模の一括操作は将来の性能改善対象
- 部分分類（スレッドの一部メールだけを選択して移動）は不可。既存の D&D と
  同じ制約
- **`bulk_commands.rs` の `plan_delete` / `plan_archive` は `mail_commands.rs`
  の同名関数の判定ロジックを意図的に複製している**（実装時点で
  `mail_commands.rs` を編集中の並行ブランチがあり、共通化すると衝突するため）。
  Sent フォルダ同期対応（バックログ項目1）で `mail_commands.rs` 側の
  LocalOnly 判定が変わった場合、`bulk_commands.rs` 側の複製が追随できず
  乖離するリスクがある。両ブランチのマージ後、リードがスタック順を決めて
  一箇所に共通化する（`mail_commands.rs` に `pub(crate)` で切り出し、
  `bulk_commands.rs` から呼ぶ形が妥当）
