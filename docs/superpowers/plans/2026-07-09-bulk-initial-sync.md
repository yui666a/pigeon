# 初回同期の大量取り込み（Bulk Initial Sync）実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 初回同期を既定5,000件のバッチ取り込みに変え、進捗表示・中断再開・大量件数でも固まらない一覧表示を実現する。

**Architecture:** IMAPからUID一覧だけを軽量取得し、100件単位・古い順に FETCH→DB挿入→進捗イベント発火 を繰り返す。差分同期も同一経路に統一。フロントは `sync-progress` イベントを購読してサイドバー下部に進捗を出し、一覧は描画のみ200件ページングする。

**Tech Stack:** Rust / async-imap / rusqlite / tauri Emitter / React 19 / Zustand 5 / Vitest + RTL

**Spec:** `docs/superpowers/specs/2026-07-09-bulk-initial-sync-design.md`（このプランの正）

## Global Constraints

- 初回同期の既定件数 **5000**（`settings.initial_sync_limit`、設定UIは作らない）
- バッチサイズ **100**（`SYNC_BATCH_SIZE` 定数）
- **古い順（UID昇順）に処理**する。これが中断再開の担保なので変更禁止
- 進捗イベント名は **`sync-progress`**、payload は `{ account_id, done, total }`（snake_case）
- 一覧の描画ページングは **200件 + 「もっと見る」**。仮想化ライブラリは導入しない
- `unwrap()` / `expect()` はテストコード以外で使用しない
- テストを先に書く（Red → Green）。Rust は `cd src-tauri && cargo test`、フロントは `pnpm test`
- 各タスク完了時に `cd src-tauri && cargo clippy -- -D warnings` が通ること（Rustタスクのみ）
- コミットは Conventional Commits（scope: `mail-sync`, `ui`）
- PR 分割: PR1 = Task 1–4（バックエンド、ブランチ `feat/bulk-sync-backend`、親 `docs/bulk-initial-sync-design`）、PR2 = Task 5–8（フロント、ブランチ `feat/bulk-sync-frontend`、親 PR1）

---

### Task 1: バッチ分割の純関数（plan_batches / uid_set）

**Files:**
- Modify: `src-tauri/src/mail_sync/imap_client.rs`
- Test: 同ファイル `#[cfg(test)] mod tests`

**Interfaces:**
- Produces:
  - `pub const SYNC_BATCH_SIZE: usize = 100;`
  - `pub(crate) fn plan_batches(uids: Vec<u32>, since_uid: u32, batch_size: usize) -> Vec<Vec<u32>>` — since_uid より大きい UID のみを昇順・重複除去し batch_size ごとに分割
  - `pub(crate) fn uid_set(batch: &[u32]) -> String` — UID FETCH 用のカンマ区切り文字列

- [ ] **Step 1: 失敗するテストを書く**

`imap_client.rs` の tests モジュール末尾に追加:

```rust
#[test]
fn test_plan_batches_filters_sorts_and_chunks() {
    // since_uid=10 より新しいものだけを昇順で 3件ずつに分割
    let uids = vec![15, 11, 30, 10, 5, 12, 20, 11]; // 逆順・重複・既取り込み分を含む
    let batches = plan_batches(uids, 10, 3);
    assert_eq!(batches, vec![vec![11, 12, 15], vec![20, 30]]);
}

#[test]
fn test_plan_batches_empty_when_nothing_new() {
    assert!(plan_batches(vec![1, 2, 3], 5, 100).is_empty());
    assert!(plan_batches(vec![], 0, 100).is_empty());
}

#[test]
fn test_plan_batches_resume_after_interruption() {
    // 中断再開: 250件目まで取り込み済み(since_uid=250)なら残りだけが対象になる
    let uids: Vec<u32> = (1..=300).collect();
    let batches = plan_batches(uids, 250, 100);
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0], (251..=300).collect::<Vec<u32>>());
}

#[test]
fn test_uid_set_joins_with_commas() {
    assert_eq!(uid_set(&[101, 102, 105]), "101,102,105");
    assert_eq!(uid_set(&[7]), "7");
}
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `cd src-tauri && cargo test mail_sync::imap_client -- --nocapture`
Expected: コンパイルエラー（`plan_batches` 未定義）

- [ ] **Step 3: 実装**

`INITIAL_SYNC_LIMIT` 定数の直後（`const INITIAL_SYNC_LIMIT` はTask 3で削除するのでこの時点では残す）に追加:

```rust
/// 同期バッチのサイズ。1バッチ分の全文のみメモリに保持する
pub const SYNC_BATCH_SIZE: usize = 100;

/// since_uid より新しい UID のみを昇順・重複除去し、batch_size ごとに分割する。
/// 古い順に処理することで、中断しても DB の max_uid がそのまま再開点になる。
pub(crate) fn plan_batches(uids: Vec<u32>, since_uid: u32, batch_size: usize) -> Vec<Vec<u32>> {
    let mut filtered: Vec<u32> = uids.into_iter().filter(|u| *u > since_uid).collect();
    filtered.sort_unstable();
    filtered.dedup();
    filtered
        .chunks(batch_size)
        .map(|chunk| chunk.to_vec())
        .collect()
}

/// UID FETCH に渡す UID セット文字列（カンマ区切り）
pub(crate) fn uid_set(batch: &[u32]) -> String {
    batch
        .iter()
        .map(|u| u.to_string())
        .collect::<Vec<_>>()
        .join(",")
}
```

- [ ] **Step 4: テストが通ることを確認**

Run: `cd src-tauri && cargo test mail_sync::imap_client -- --nocapture`
Expected: 新規4件を含め PASS

- [ ] **Step 5: コミット**

```bash
git add src-tauri/src/mail_sync/imap_client.rs
git commit -m "feat(mail-sync): 同期バッチ分割の純関数を追加"
```

---

### Task 2: settings の u32 読み出しヘルパー

**Files:**
- Modify: `src-tauri/src/db/settings.rs`
- Test: 同ファイル `#[cfg(test)] mod tests`

**Interfaces:**
- Consumes: 既存 `get_or_default(conn, key, default) -> String`
- Produces: `pub fn get_u32_or(conn: &Connection, key: &str, default: u32) -> u32` — 未設定・数値でない値は default にフォールバック

- [ ] **Step 1: 失敗するテストを書く**

`settings.rs` の tests モジュール末尾に追加:

```rust
#[test]
fn test_get_u32_or_returns_default_when_missing() {
    let conn = setup_db();
    assert_eq!(get_u32_or(&conn, "initial_sync_limit", 5000), 5000);
}

#[test]
fn test_get_u32_or_parses_stored_value() {
    let conn = setup_db();
    conn.execute(
        "INSERT INTO settings (key, value) VALUES ('initial_sync_limit', '300')",
        [],
    )
    .unwrap();
    assert_eq!(get_u32_or(&conn, "initial_sync_limit", 5000), 300);
}

#[test]
fn test_get_u32_or_falls_back_on_invalid_value() {
    let conn = setup_db();
    conn.execute(
        "INSERT INTO settings (key, value) VALUES ('initial_sync_limit', 'abc')",
        [],
    )
    .unwrap();
    assert_eq!(get_u32_or(&conn, "initial_sync_limit", 5000), 5000);
}
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `cd src-tauri && cargo test db::settings -- --nocapture`
Expected: コンパイルエラー（`get_u32_or` 未定義）

- [ ] **Step 3: 実装**

`get_or_default` の直後に追加:

```rust
/// `key` の値を u32 として読む。未設定・数値でない場合は `default`。
pub fn get_u32_or(conn: &Connection, key: &str, default: u32) -> u32 {
    get_or_default(conn, key, &default.to_string())
        .parse()
        .unwrap_or(default)
}
```

- [ ] **Step 4: テストが通ることを確認**

Run: `cd src-tauri && cargo test db::settings -- --nocapture`
Expected: 全 PASS

- [ ] **Step 5: コミット**

```bash
git add src-tauri/src/db/settings.rs
git commit -m "feat(db): settingsのu32読み出しヘルパーを追加"
```

---

### Task 3: fetch_mails_batched（IMAP バッチ取得）

**Files:**
- Modify: `src-tauri/src/mail_sync/imap_client.rs`（`fetch_mails_since_uid` を置き換え）

**Interfaces:**
- Consumes: Task 1 の `plan_batches` / `uid_set` / `SYNC_BATCH_SIZE`
- Produces:
  - `pub struct SyncProgress { pub done: usize, pub total: usize }`
  - `pub async fn fetch_mails_batched(session: &mut ImapSession, folder: &str, since_uid: u32, initial_limit: u32, on_batch: impl FnMut(Vec<(u32, Vec<u8>)>, SyncProgress) -> Result<(), AppError>) -> Result<usize, AppError>` — 取り込み対象の総件数を返す。バッチごとに on_batch を呼ぶ
  - `fetch_mails_since_uid` と `INITIAL_SYNC_LIMIT` は**削除**（Task 4 で呼び出し側も置き換わる。このタスクの時点では呼び出し側が壊れるため、Task 3 と Task 4 は同一コミットにする）

このタスクはIMAPサーバとの境界コードのためユニットテストは書かない（分割・順序のロジックは Task 1 で担保済み）。コンパイルと既存テストの回帰で検証する。

- [ ] **Step 1: fetch_mails_batched を実装し fetch_mails_since_uid を削除**

`imap_client.rs` の `fetch_mails_since_uid` 関数全体と `INITIAL_SYNC_LIMIT` 定数を削除し、以下に置き換える:

```rust
/// 同期の進捗（on_batch コールバックに渡す）
pub struct SyncProgress {
    pub done: usize,
    pub total: usize,
}

/// since_uid より新しいメールを、UID一覧の軽量取得 → SYNC_BATCH_SIZE 件ずつの
/// バッチ FETCH で取り込む。バッチごとに on_batch(そのバッチの生メール, 進捗) を呼ぶ。
/// 古い順（UID昇順）に処理するため、途中で中断しても DB の max_uid が再開点になる。
/// 戻り値は取り込み対象の総件数。
pub async fn fetch_mails_batched(
    session: &mut ImapSession,
    folder: &str,
    since_uid: u32,
    initial_limit: u32,
    mut on_batch: impl FnMut(Vec<(u32, Vec<u8>)>, SyncProgress) -> Result<(), AppError>,
) -> Result<usize, AppError> {
    let mailbox = session
        .select(folder)
        .await
        .map_err(|e| AppError::Imap(format!("Select folder failed: {}", e)))?;

    // 対象の UID 一覧のみを軽量取得（本文なし）
    let uids: Vec<u32> = if since_uid == 0 {
        // 初回同期: 直近 initial_limit 件のシーケンス範囲から UID を得る
        let total = mailbox.exists;
        if total == 0 {
            return Ok(0);
        }
        let start = if total > initial_limit {
            total - initial_limit + 1
        } else {
            1
        };
        let messages: Vec<_> = session
            .fetch(&format!("{}:*", start), "(UID)")
            .await
            .map_err(|e| AppError::Imap(format!("UID list fetch failed: {}", e)))?
            .try_collect()
            .await
            .map_err(|e| AppError::Imap(format!("UID list stream failed: {}", e)))?;
        messages.iter().filter_map(|m| m.uid).collect()
    } else {
        // 差分同期: since_uid より新しい範囲の UID を得る
        let messages: Vec<_> = session
            .uid_fetch(&format!("{}:*", since_uid + 1), "(UID)")
            .await
            .map_err(|e| AppError::Imap(format!("UID list fetch failed: {}", e)))?
            .try_collect()
            .await
            .map_err(|e| AppError::Imap(format!("UID list stream failed: {}", e)))?;
        messages.iter().filter_map(|m| m.uid).collect()
    };

    let batches = plan_batches(uids, since_uid, SYNC_BATCH_SIZE);
    let total: usize = batches.iter().map(|b| b.len()).sum();

    let mut done = 0usize;
    for batch in batches {
        let messages: Vec<_> = session
            .uid_fetch(&uid_set(&batch), "(UID RFC822)")
            .await
            .map_err(|e| AppError::Imap(format!("Batch fetch failed: {}", e)))?
            .try_collect()
            .await
            .map_err(|e| AppError::Imap(format!("Batch stream failed: {}", e)))?;

        let mut mails = Vec::with_capacity(messages.len());
        for msg in &messages {
            if let (Some(uid), Some(body)) = (msg.uid, msg.body()) {
                if uid > since_uid {
                    mails.push((uid, body.to_vec()));
                }
            }
        }
        done += batch.len();
        on_batch(mails, SyncProgress { done, total })?;
    }
    Ok(total)
}
```

- [ ] **Step 2: Task 4 と合わせてコンパイル確認するため、ここではコミットしない**

Note: この時点では `mail_commands.rs` が旧関数を呼んでいてビルドが通らない。**そのまま Task 4 に進む。**

---

### Task 4: sync_account のバッチ化と進捗 emit

**Files:**
- Modify: `src-tauri/src/commands/mail_commands.rs`

**Interfaces:**
- Consumes: Task 1–3 の `fetch_mails_batched` / `SyncProgress`、`settings::get_u32_or`
- Produces:
  - `sync_account` command の引数に `app: AppHandle` が加わる（フロントの invoke 呼び出しは変更不要）
  - Tauri イベント `sync-progress`、payload `SyncProgressEvent { account_id: String, done: usize, total: usize }`
  - `sync_account_inner(state, secure_store, account_id, on_progress: impl FnMut(usize, usize)) -> Result<u32, AppError>`

- [ ] **Step 1: mail_commands.rs を書き換える**

先頭の use を変更:

```rust
use tauri::{AppHandle, Emitter, State};
```

`sync_account` command と `sync_account_inner` を以下に置き換える（`resolve_imap_credentials` は変更しない）:

```rust
/// sync-progress イベントの payload
#[derive(Clone, serde::Serialize)]
struct SyncProgressEvent {
    account_id: String,
    done: usize,
    total: usize,
}

#[tauri::command]
pub async fn sync_account(
    app: AppHandle,
    state: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    account_id: String,
) -> Result<u32, AppError> {
    sync_account_inner(&state, &secure_store.0, &account_id, |done, total| {
        // 進捗はベストエフォート（emit 失敗で同期は止めない）
        let _ = app.emit(
            "sync-progress",
            SyncProgressEvent {
                account_id: account_id.clone(),
                done,
                total,
            },
        );
    })
    .await
}

async fn sync_account_inner(
    state: &DbState,
    secure_store: &crate::secure_store::SecureStore,
    account_id: &str,
    mut on_progress: impl FnMut(usize, usize),
) -> Result<u32, AppError> {
    let (account, max_uid, initial_limit) = {
        let conn = state.0.lock().map_err(AppError::lock_err)?;
        let account = accounts::get_account(&conn, account_id)?;
        let max_uid = mails::get_max_uid(&conn, account_id, "INBOX")?;
        let initial_limit =
            crate::db::settings::get_u32_or(&conn, "initial_sync_limit", 5000);
        (account, max_uid, initial_limit)
    };

    let (auth_type, username, credential) =
        resolve_imap_credentials(&account, secure_store).await?;

    let mut session = imap_client::connect(
        &account.imap_host,
        account.imap_port,
        &auth_type,
        &username,
        &credential,
    )
    .await?;

    let mut count = 0u32;
    let fetch_result = imap_client::fetch_mails_batched(
        &mut session,
        "INBOX",
        max_uid,
        initial_limit,
        |batch, progress| {
            // バッチ単位でロックを取り、挿入してから進捗を通知する
            {
                let conn = state.0.lock().map_err(AppError::lock_err)?;
                for (uid, body) in &batch {
                    if let Some(mail) = mime_parser::parse_mime(body, account_id, "INBOX", *uid) {
                        mails::insert_mail(&conn, &mail)?;
                        count += 1;
                    }
                }
            }
            on_progress(progress.done, progress.total);
            Ok(())
        },
    )
    .await;

    if let Err(e) = session.logout().await {
        eprintln!("[warn] IMAP logout failed: {}", e);
    }
    fetch_result?;
    Ok(count)
}
```

- [ ] **Step 2: ビルドとテスト・clippy を確認**

Run: `cd src-tauri && cargo test && cargo clippy -- -D warnings`
Expected: 全 PASS / warning なし（`db::settings` を `use` していない場合はフルパス呼び出しなので追加 use 不要）

- [ ] **Step 3: Task 3 とまとめてコミット**

```bash
git add src-tauri/src/mail_sync/imap_client.rs src-tauri/src/commands/mail_commands.rs
git commit -m "feat(mail-sync): 同期をバッチ化し初回5000件と進捗イベントに対応"
```

---

### Task 5: mailStore の進捗購読と順次反映

**Files:**
- Modify: `src/stores/mailStore.ts`
- Test: `src/__tests__/stores/mailStore.test.ts`（追記 + mock 追加）

**Interfaces:**
- Consumes: Tauri イベント `sync-progress`（Task 4）
- Produces（コンポーネントが使う状態とアクション）:
  - `syncProgress: { account_id: string; done: number; total: number } | null`
  - `initSyncListener: () => Promise<() => void>` — `sync-progress` を購読。500件ごと・完了時に一覧を再取得
  - `syncAccount` は完了時（成功・失敗とも）に `syncProgress: null` へ戻す

- [ ] **Step 1: 失敗するテストを書く**

`src/__tests__/stores/mailStore.test.ts` に、ファイル先頭の mock 群へ event の mock を追加する（既存の `vi.mock("@tauri-apps/api/core", ...)` の隣）:

```typescript
let syncProgressHandler: ((event: { payload: unknown }) => void) | null = null;
const mockUnlisten = vi.fn();
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn((name: string, handler: (event: { payload: unknown }) => void) => {
    if (name === "sync-progress") syncProgressHandler = handler;
    return Promise.resolve(mockUnlisten);
  }),
}));
```

describe ブロックを末尾に追加（既存 beforeEach の `setState` に `syncProgress: null,` を追加すること）:

```typescript
describe("sync progress", () => {
  it("updates syncProgress on sync-progress events", async () => {
    await useMailStore.getState().initSyncListener();
    syncProgressHandler!({
      payload: { account_id: "acc1", done: 100, total: 5000 },
    });
    expect(useMailStore.getState().syncProgress).toEqual({
      account_id: "acc1",
      done: 100,
      total: 5000,
    });
  });

  it("refreshes lists every 500 mails and at completion, not on every batch", async () => {
    mockInvoke.mockResolvedValue([]);
    await useMailStore.getState().initSyncListener();

    syncProgressHandler!({ payload: { account_id: "acc1", done: 100, total: 1200 } });
    expect(mockInvoke).not.toHaveBeenCalledWith("get_threads", expect.anything());

    syncProgressHandler!({ payload: { account_id: "acc1", done: 500, total: 1200 } });
    expect(mockInvoke).toHaveBeenCalledWith("get_threads", {
      accountId: "acc1",
      folder: "INBOX",
    });
    expect(mockInvoke).toHaveBeenCalledWith("get_unclassified_mails", {
      accountId: "acc1",
    });

    mockInvoke.mockClear();
    mockInvoke.mockResolvedValue([]);
    syncProgressHandler!({ payload: { account_id: "acc1", done: 1200, total: 1200 } });
    expect(mockInvoke).toHaveBeenCalledWith("get_threads", {
      accountId: "acc1",
      folder: "INBOX",
    });
  });

  it("clears syncProgress when syncAccount finishes", async () => {
    mockInvoke.mockResolvedValue(3);
    useMailStore.setState({
      syncProgress: { account_id: "acc1", done: 100, total: 200 },
    });
    await useMailStore.getState().syncAccount("acc1");
    expect(useMailStore.getState().syncProgress).toBeNull();
  });
});
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `pnpm test mailStore`
Expected: FAIL（`initSyncListener` / `syncProgress` 未定義）

- [ ] **Step 3: 実装**

`src/stores/mailStore.ts` に追加。import:

```typescript
import { listen } from "@tauri-apps/api/event";
```

型定義（`MailState` の上）:

```typescript
interface SyncProgress {
  account_id: string;
  done: number;
  total: number;
}
```

`MailState` interface に追加:

```typescript
  syncProgress: SyncProgress | null;
  initSyncListener: () => Promise<() => void>;
```

ストア本体: 初期値 `syncProgress: null,` を追加し、`syncAccount` の成功パスを `set({ syncing: false, syncProgress: null });`、catch パスを `set({ error: errorMsg, syncing: false, needsReauth: isReauth, syncProgress: null });` に変更。アクションを追加:

```typescript
  initSyncListener: async () => {
    const unlisten = await listen<SyncProgress>("sync-progress", (event) => {
      const p = event.payload;
      set({ syncProgress: p });
      // 一覧への順次反映は500件ごと（=5バッチに1回）と完了時のみ。
      // 毎バッチのDB再読込を避ける
      if (p.done % 500 === 0 || p.done === p.total) {
        void get().fetchThreads(p.account_id, "INBOX");
        void get().fetchUnclassified(p.account_id);
      }
    });
    return unlisten;
  },
```

- [ ] **Step 4: テストが通ることを確認**

Run: `pnpm test mailStore`
Expected: 既存 + 新規3件すべて PASS

- [ ] **Step 5: コミット**

```bash
git add src/stores/mailStore.ts src/__tests__/stores/mailStore.test.ts
git commit -m "feat(ui): mailStoreに同期進捗の購読と順次反映を追加"
```

---

### Task 6: SyncIndicator（サイドバー下部の進捗表示）

**Files:**
- Create: `src/components/sidebar/SyncIndicator.tsx`
- Modify: `src/components/sidebar/Sidebar.tsx`（`<ScanIndicator />` の直前に配置）
- Test: `src/__tests__/SyncIndicator.test.tsx`（新規）

**Interfaces:**
- Consumes: Task 5 の `syncProgress` / `initSyncListener`
- Produces: `SyncIndicator` コンポーネント（props なし）

- [ ] **Step 1: 失敗するテストを書く**

`src/__tests__/SyncIndicator.test.tsx`:

```typescript
import { render, screen } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { SyncIndicator } from "../components/sidebar/SyncIndicator";
import { useMailStore } from "../stores/mailStore";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

describe("SyncIndicator", () => {
  beforeEach(() => {
    useMailStore.setState({ syncProgress: null });
  });

  it("renders nothing when no sync is in progress", () => {
    const { container } = render(<SyncIndicator />);
    expect(container).toBeEmptyDOMElement();
  });

  it("shows progress with thousands separators while syncing", () => {
    useMailStore.setState({
      syncProgress: { account_id: "acc1", done: 1200, total: 5000 },
    });
    render(<SyncIndicator />);
    expect(screen.getByText(/メール同期中… 1,200 \/ 5,000/)).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `pnpm test SyncIndicator`
Expected: FAIL（コンポーネント未定義）

- [ ] **Step 3: 実装**

`src/components/sidebar/SyncIndicator.tsx`:

```tsx
import { useEffect } from "react";
import { useMailStore } from "../../stores/mailStore";

export function SyncIndicator() {
  const syncProgress = useMailStore((s) => s.syncProgress);
  const initSyncListener = useMailStore((s) => s.initSyncListener);

  useEffect(() => {
    const promise = initSyncListener();
    return () => {
      promise.then((unlisten) => unlisten());
    };
  }, [initSyncListener]);

  if (!syncProgress) return null;

  return (
    <div className="border-t px-4 py-1.5 text-xs text-gray-500">
      メール同期中… {syncProgress.done.toLocaleString()} /{" "}
      {syncProgress.total.toLocaleString()}
    </div>
  );
}
```

`Sidebar.tsx` に import と配置を追加:

```tsx
import { SyncIndicator } from "./SyncIndicator";
```

`<ScanIndicator />` の直前の行に:

```tsx
      <SyncIndicator />
```

- [ ] **Step 4: テストが通ることを確認**

Run: `pnpm test`
Expected: 新規2件を含め全 PASS（Sidebar.test の既存テストが壊れていないこと）

- [ ] **Step 5: コミット**

```bash
git add src/components/sidebar/SyncIndicator.tsx src/components/sidebar/Sidebar.tsx \
        src/__tests__/SyncIndicator.test.tsx
git commit -m "feat(ui): サイドバー下部にメール同期の進捗表示を追加"
```

---

### Task 7: useDisplayLimit フック（描画ページング）

**Files:**
- Create: `src/hooks/useDisplayLimit.ts`
- Test: `src/__tests__/hooks/useDisplayLimit.test.ts`（新規。`src/__tests__/hooks/` ディレクトリも新規）

**Interfaces:**
- Produces:
  - `useDisplayLimit<T>(items: T[], resetKey: unknown): { visible: T[]; hasMore: boolean; remaining: number; showMore: () => void }`
  - 初期表示 200 件。`showMore()` で +200。`resetKey` が変わると 200 に戻る

- [ ] **Step 1: 失敗するテストを書く**

`src/__tests__/hooks/useDisplayLimit.test.ts`:

```typescript
import { renderHook, act } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { useDisplayLimit } from "../../hooks/useDisplayLimit";

const items = (n: number) => Array.from({ length: n }, (_, i) => i);

describe("useDisplayLimit", () => {
  it("shows at most 200 items initially", () => {
    const { result } = renderHook(() => useDisplayLimit(items(250), "a"));
    expect(result.current.visible).toHaveLength(200);
    expect(result.current.hasMore).toBe(true);
    expect(result.current.remaining).toBe(50);
  });

  it("shows all items when 200 or fewer", () => {
    const { result } = renderHook(() => useDisplayLimit(items(200), "a"));
    expect(result.current.visible).toHaveLength(200);
    expect(result.current.hasMore).toBe(false);
  });

  it("showMore reveals 200 more items", () => {
    const { result } = renderHook(() => useDisplayLimit(items(450), "a"));
    act(() => result.current.showMore());
    expect(result.current.visible).toHaveLength(400);
    act(() => result.current.showMore());
    expect(result.current.visible).toHaveLength(450);
    expect(result.current.hasMore).toBe(false);
  });

  it("resets to 200 when resetKey changes", () => {
    const { result, rerender } = renderHook(
      ({ key }) => useDisplayLimit(items(450), key),
      { initialProps: { key: "a" } },
    );
    act(() => result.current.showMore());
    expect(result.current.visible).toHaveLength(400);
    rerender({ key: "b" });
    expect(result.current.visible).toHaveLength(200);
  });
});
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `pnpm test useDisplayLimit`
Expected: FAIL（モジュール未定義）

- [ ] **Step 3: 実装**

`src/hooks/useDisplayLimit.ts`:

```typescript
import { useCallback, useEffect, useState } from "react";

const PAGE_SIZE = 200;

/**
 * 大量リストの描画ページング。データは全件持ち、描画だけを
 * 先頭 PAGE_SIZE 件 + 「もっと見る」で切る（仮想化ライブラリは使わない）。
 */
export function useDisplayLimit<T>(items: T[], resetKey: unknown) {
  const [limit, setLimit] = useState(PAGE_SIZE);

  useEffect(() => {
    setLimit(PAGE_SIZE);
  }, [resetKey]);

  const showMore = useCallback(() => setLimit((l) => l + PAGE_SIZE), []);

  return {
    visible: items.slice(0, limit),
    hasMore: items.length > limit,
    remaining: Math.max(0, items.length - limit),
    showMore,
  };
}
```

- [ ] **Step 4: テストが通ることを確認**

Run: `pnpm test useDisplayLimit`
Expected: 4件 PASS

- [ ] **Step 5: コミット**

```bash
git add src/hooks/useDisplayLimit.ts src/__tests__/hooks/useDisplayLimit.test.ts
git commit -m "feat(ui): 描画ページング用のuseDisplayLimitフックを追加"
```

---

### Task 8: ThreadList / UnclassifiedList にページング適用 + 最終確認

**Files:**
- Modify: `src/components/thread-list/ThreadList.tsx`
- Modify: `src/components/thread-list/UnclassifiedList.tsx`
- Test: `src/__tests__/ThreadListPaging.test.tsx`（新規）

**Interfaces:**
- Consumes: Task 7 の `useDisplayLimit`

- [ ] **Step 1: 失敗するテストを書く**

`src/__tests__/ThreadListPaging.test.tsx`:

```typescript
import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { ThreadList } from "../components/thread-list/ThreadList";
import { useAccountStore } from "../stores/accountStore";
import { useMailStore } from "../stores/mailStore";
import type { Thread } from "../types/mail";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(() => Promise.resolve([])),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

const thread = (i: number): Thread => ({
  thread_id: `t${i}`,
  subject: `件名 ${i}`,
  last_date: "2026-07-09T00:00:00Z",
  mail_count: 1,
  from_addrs: ["a@example.com"],
  mails: [],
});

describe("ThreadList paging", () => {
  beforeEach(() => {
    useAccountStore.setState({ selectedAccountId: "acc1" });
    useMailStore.setState({
      threads: Array.from({ length: 250 }, (_, i) => thread(i)),
      syncing: false,
      needsReauth: false,
      selectedThread: null,
      syncProgress: null,
    });
  });

  it("renders only the first 200 threads with a show-more button", () => {
    render(<ThreadList viewMode="project" />);
    expect(screen.getByText("件名 0")).toBeInTheDocument();
    expect(screen.getByText("件名 199")).toBeInTheDocument();
    expect(screen.queryByText("件名 200")).not.toBeInTheDocument();
    expect(screen.getByText(/もっと見る（残り 50 件）/)).toBeInTheDocument();
  });

  it("reveals more threads on click", () => {
    render(<ThreadList viewMode="project" />);
    fireEvent.click(screen.getByText(/もっと見る/));
    expect(screen.getByText("件名 249")).toBeInTheDocument();
    expect(screen.queryByText(/もっと見る/)).not.toBeInTheDocument();
  });
});
```

- [ ] **Step 2: テストが失敗することを確認**

Run: `pnpm test ThreadListPaging`
Expected: FAIL（「もっと見る」が存在しない / 250件全部描画される）

- [ ] **Step 3: ThreadList に適用**

`ThreadList.tsx` に import を追加:

```typescript
import { useDisplayLimit } from "../../hooks/useDisplayLimit";
```

コンポーネント内（early return より前、hooks 規約上必ず先頭側）に追加:

```typescript
  const { visible, hasMore, remaining, showMore } = useDisplayLimit(
    threads,
    `${viewMode}:${selectedProjectId ?? ""}:${selectedAccountId ?? ""}`,
  );
```

描画部を差し替え:

```tsx
  return (
    <div className="h-full overflow-y-auto">
      {visible.map((thread) => (
        <ThreadItem
          key={thread.thread_id}
          thread={thread}
          selected={selectedThread?.thread_id === thread.thread_id}
          onClick={() => selectThread(thread)}
        />
      ))}
      {hasMore && (
        <button
          onClick={showMore}
          className="w-full py-2 text-sm text-blue-600 hover:bg-gray-50"
        >
          もっと見る（残り {remaining.toLocaleString()} 件）
        </button>
      )}
    </div>
  );
```

- [ ] **Step 4: UnclassifiedList に適用**

`UnclassifiedList.tsx` に import を追加:

```typescript
import { useDisplayLimit } from "../../hooks/useDisplayLimit";
```

コンポーネント内（`if (!selectedAccountId) return null;` より**前**）に追加:

```typescript
  const {
    visible: visibleMails,
    hasMore,
    remaining,
    showMore,
  } = useDisplayLimit(unclassifiedMails, selectedAccountId);
```

`unclassifiedMails.map` の描画ブロックを差し替え:

```tsx
      {unclassifiedMails.length > 0 && (
        <div className="max-h-48 overflow-y-auto">
          {visibleMails.map((mail) => (
            <MailDragItem
              key={mail.id}
              mail={mail}
              onClick={() => handleMailClick(mail)}
            />
          ))}
          {hasMore && (
            <button
              onClick={showMore}
              className="w-full py-2 text-xs text-blue-600 hover:bg-gray-50"
            >
              もっと見る（残り {remaining.toLocaleString()} 件）
            </button>
          )}
        </div>
      )}
```

- [ ] **Step 5: テストが通ることを確認**

Run: `pnpm test`
Expected: 新規2件を含め全 PASS

- [ ] **Step 6: 最終確認（全体）**

Run: `pnpm test && pnpm build && cd src-tauri && cargo test && cargo clippy -- -D warnings`
Expected: すべて成功

- [ ] **Step 7: コミット**

```bash
git add src/components/thread-list/ThreadList.tsx src/components/thread-list/UnclassifiedList.tsx \
        src/__tests__/ThreadListPaging.test.tsx
git commit -m "feat(ui): スレッド一覧と未分類一覧に描画ページングを追加"
```
