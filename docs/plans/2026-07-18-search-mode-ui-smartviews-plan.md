# 検索モード切替 UI ＋ スマートビュー（保存検索） 実装計画

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 検索バーに 文字列/ベクトル のモードトグルを付けて `search_mails` / `semantic_search` を呼び分け、検索を名前付きで保存してサイドバーの「スマートビュー」セクションからワンクリック再実行できるようにする。

**Architecture:** モードは `searchStore` に持ち localStorage（`pigeon.searchMode`）で永続化。スマートビューは新テーブル `saved_searches`（migration v19）＋ projects と同型の直 CRUD（dispatch バスは使わない — 特権メール操作ではない単純データ保存のため）＋ `ProjectTree` と同型のサイドバーセクション。検索結果画面に「この検索を保存」と 関連度/日付 の並び替えトグルを追加する。

**Tech Stack:** React 19 / TypeScript / Zustand 5 / Tailwind v4 / Vitest + RTL（fireEvent 流儀）/ Rust + rusqlite / 既存 `semantic_search` command（PR #189）

**設計書:** `docs/design/2026-07-17-search-enhancement-design.md` の「Phase 3: 検索モード切替 UI + スマートビュー」節（承認済み）

## Global Constraints

- `unwrap()` / `expect()` はテストコード以外で使用しない。エラーは `crate::error::AppError`
- TDD: 各タスクは失敗するテストを先に書く（Red → Green）
- コミットは Conventional Commits、1コミット=1意図。PRタイトル・本文に内部フェーズ名（「Phase 3」等)を使わない
- マイグレーションは **v19**（#188 が v18 を消費済み。他ブランチに v19 が現れていたら次の空き番号に読み替え）
- モードの正準値は **`"fulltext"` / `"semantic"`**（設計書の SQL CHECK と searchStore の型を同じ文字列で統一）
- 永続化は既存の localStorage 慣例（`NotificationToggle` と同じ。キーは `pigeon.searchMode`。設定テーブル移行はバックログ #16 で別途）
- フロントの型: `any` 禁止、invoke レスポンスに型必須。TS interface は Rust struct と snake_case のまま一致させる（既存 `types/project.ts` の流儀）
- テスト実行: `cd src-tauri && cargo test` / `pnpm test`。コンポーネントテストは `src/__tests__/*.test.tsx`（フラット配置）、ストアテストは `src/__tests__/stores/*.test.ts`
- `cargo fmt` は触ったファイルのみコミットに含める。既存 clippy 負債は範囲外
- **前提**: PR #188/#189 マージ後の main から分岐

## PR 構成

| PR | ブランチ | 内容 | タスク |
|---|---|---|---|
| C | `feat/search-mode-toggle`（base: main） | モード切替＋呼び分け＋永続化 | Task 1〜2 |
| D | `feat/smart-views`（base: PR C、Stacked） | saved_searches＋スマートビュー UI | Task 3〜7 |

## ファイル構成

| ファイル | 役割 |
|---|---|
| Modify: `src/types/search.ts`（無ければ Create） | `SearchMode` 型 |
| Modify: `src/stores/searchStore.ts` | `mode` 状態・永続化・`search()` の呼び分け |
| Create: `src/components/sidebar/SearchModeToggle.tsx` | 文字列/ベクトル トグル |
| Modify: `src/components/sidebar/Sidebar.tsx` | トグル配置（SearchBar 直下） |
| Modify: `src-tauri/src/db/migrations.rs` | migrate_v19（saved_searches） |
| Create: `src-tauri/src/db/saved_searches.rs` | CRUD（projects.rs と同型） |
| Create: `src-tauri/src/models/saved_search.rs` | `SavedSearch` / `CreateSavedSearchRequest` |
| Create: `src-tauri/src/commands/saved_search_commands.rs` | Tauri commands（project_commands と同型） |
| Modify: `src-tauri/src/lib.rs` ほか mod 宣言 | command 登録 |
| Create: `src/types/savedSearch.ts` / `src/api/savedSearchApi.ts` / `src/stores/savedSearchStore.ts` | フロント CRUD 一式 |
| Create: `src/components/sidebar/SmartViewList.tsx` | サイドバー「スマートビュー」セクション |
| Modify: `src/components/thread-list/SearchResults.tsx` | 「この検索を保存」＋並び替えトグル |

---

## Task 1: searchStore にモードを追加（永続化＋呼び分け）

**Files:**
- Modify: `src/types/search.ts`（`SearchResult` がある型ファイル。無ければ Create し、`SearchResult` の場所は変えない）
- Modify: `src/stores/searchStore.ts`
- Test: `src/__tests__/searchStore.test.ts`（既存に追記）

**Interfaces:**
- Consumes: `searchApi.searchMails` / `searchApi.semanticSearch`（PR #189 で追加済み、シグネチャ同一: `(accountId: string, query: string) => Promise<SearchResult[]>`）
- Produces:
  - `export type SearchMode = "fulltext" | "semantic"`（types/search.ts）
  - `export const SEARCH_MODE_KEY = "pigeon.searchMode"`（searchStore.ts）
  - searchStore 追加分: `mode: SearchMode` / `setMode(mode: SearchMode): void`（クエリがあれば現在アカウントで再検索はしない — 呼び出し側の責務にせず、**setMode は状態変更と永続化のみ**。再検索は Task 2 のトグルが `search()` を呼んで行う）
  - `search(accountId, query)` は `mode === "semantic"` なら `searchApi.semanticSearch`、それ以外は `searchApi.searchMails` を呼ぶ

- [ ] **Step 1: 失敗するテストを追記**

`src/__tests__/searchStore.test.ts` に追記（既存の `vi.mock("@tauri-apps/api/core", ...)` 形式を `mockInvoke` 参照型に揃える必要があれば既存テストを壊さない範囲で調整）:

```typescript
import { useSearchStore, SEARCH_MODE_KEY } from "../stores/searchStore";

const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

describe("search mode", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
    useSearchStore.setState({ mode: "fulltext", query: "", results: [], searching: false, selectedIndex: -1 });
    mockInvoke.mockResolvedValue([]);
  });

  it("デフォルトは fulltext で search_mails を呼ぶ", async () => {
    await useSearchStore.getState().search("acc1", "照明");
    expect(mockInvoke).toHaveBeenCalledWith("search_mails", { accountId: "acc1", query: "照明" });
  });

  it("semantic モードでは semantic_search を呼ぶ", async () => {
    useSearchStore.getState().setMode("semantic");
    await useSearchStore.getState().search("acc1", "照明");
    expect(mockInvoke).toHaveBeenCalledWith("semantic_search", { accountId: "acc1", query: "照明" });
  });

  it("setMode は localStorage に永続化する", () => {
    useSearchStore.getState().setMode("semantic");
    expect(localStorage.getItem(SEARCH_MODE_KEY)).toBe("semantic");
    useSearchStore.getState().setMode("fulltext");
    expect(localStorage.getItem(SEARCH_MODE_KEY)).toBe("fulltext");
  });

  it("不正な保存値は fulltext にフォールバックする", () => {
    localStorage.setItem(SEARCH_MODE_KEY, "garbage");
    expect(useSearchStore.getState().loadPersistedMode()).toBe("fulltext");
    localStorage.setItem(SEARCH_MODE_KEY, "semantic");
    expect(useSearchStore.getState().loadPersistedMode()).toBe("semantic");
  });
});
```

- [ ] **Step 2: Red を確認**

Run: `pnpm test searchStore`
Expected: FAIL（mode / setMode / loadPersistedMode 未定義）

- [ ] **Step 3: 実装**

`src/types/search.ts` に追加:

```typescript
export type SearchMode = "fulltext" | "semantic";
```

`src/stores/searchStore.ts`（既存 state/actions は温存し追記。初期 mode は localStorage から読む）:

```typescript
import type { SearchMode } from "../types/search";

/** 保存先は既存の通知トグルと統一（localStorage。設定テーブル移行はバックログ #16） */
export const SEARCH_MODE_KEY = "pigeon.searchMode";

function readPersistedMode(): SearchMode {
  const v = localStorage.getItem(SEARCH_MODE_KEY);
  return v === "semantic" ? "semantic" : "fulltext";
}

// ストア定義に追加:
//   mode: readPersistedMode(),
//   setMode: (mode) => { localStorage.setItem(SEARCH_MODE_KEY, mode); set({ mode }); },
//   loadPersistedMode: () => readPersistedMode(),
// search() 内の呼び分け:
//   const api = get().mode === "semantic" ? searchApi.semanticSearch : searchApi.searchMails;
//   const results = await api(accountId, query);
```

（エラー処理は既存 `search()` の `useErrorStore` 経路をそのまま共用。Ollama 未起動時の `semantic_search` エラーもこの経路でユーザーに見える）

- [ ] **Step 4: Green を確認**

Run: `pnpm test searchStore && pnpm tsc --noEmit`
Expected: PASS

- [ ] **Step 5: コミット**

```bash
git add src/types/search.ts src/stores/searchStore.ts src/__tests__/searchStore.test.ts
git commit -m "feat(search): searchStoreに検索モード(fulltext/semantic)と永続化・呼び分けを追加"
```

---

## Task 2: SearchModeToggle コンポーネントと Sidebar 配線

**Files:**
- Create: `src/components/sidebar/SearchModeToggle.tsx`
- Modify: `src/components/sidebar/Sidebar.tsx`（`<SearchBar ... />` の直下に配置）
- Test: `src/__tests__/SearchModeToggle.test.tsx`

**Interfaces:**
- Consumes: `useSearchStore` の `mode` / `setMode` / `search` / `query`、`useUiStore` の viewMode（Sidebar 既存の `handleSearch` と同じ流儀）
- Produces: `SearchModeToggle`（props なし。ストア直結 — SearchBar が dumb なのと対照的だが、トグルは検索実行まで担うため）

- [ ] **Step 1: 失敗するテストを書く**

```tsx
import { render, screen, fireEvent } from "@testing-library/react";
import { SearchModeToggle } from "../components/sidebar/SearchModeToggle";
import { useSearchStore } from "../stores/searchStore";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn().mockResolvedValue([]) }));

describe("SearchModeToggle", () => {
  beforeEach(() => {
    localStorage.clear();
    useSearchStore.setState({ mode: "fulltext", query: "", results: [], searching: false, selectedIndex: -1 });
  });

  it("両モードのボタンを表示し現在モードを強調する", () => {
    render(<SearchModeToggle />);
    expect(screen.getByRole("button", { name: "文字列" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "ベクトル" })).toHaveAttribute("aria-pressed", "false");
  });

  it("クリックでモードを切り替える", () => {
    render(<SearchModeToggle />);
    fireEvent.click(screen.getByRole("button", { name: "ベクトル" }));
    expect(useSearchStore.getState().mode).toBe("semantic");
  });

  it("検索中のクエリがあればモード切替で再検索する", () => {
    const searchSpy = vi.fn();
    useSearchStore.setState({ query: "照明", search: searchSpy });
    render(<SearchModeToggle accountId="acc1" />);
    fireEvent.click(screen.getByRole("button", { name: "ベクトル" }));
    expect(searchSpy).toHaveBeenCalledWith("acc1", "照明");
  });
});
```

（3本目のため props は `accountId?: string | null` を受ける。Sidebar から `selectedAccountId` を渡す）

- [ ] **Step 2: Red を確認**

Run: `pnpm test SearchModeToggle`
Expected: FAIL（コンポーネント未定義）

- [ ] **Step 3: 実装**

```tsx
import { useSearchStore } from "../../stores/searchStore";
import type { SearchMode } from "../../types/search";

interface SearchModeToggleProps {
  accountId?: string | null;
}

const MODES: { value: SearchMode; label: string }[] = [
  { value: "fulltext", label: "文字列" },
  { value: "semantic", label: "ベクトル" },
];

export function SearchModeToggle({ accountId }: SearchModeToggleProps) {
  const mode = useSearchStore((s) => s.mode);
  const setMode = useSearchStore((s) => s.setMode);
  const query = useSearchStore((s) => s.query);
  const search = useSearchStore((s) => s.search);

  const handleSelect = (next: SearchMode) => {
    if (next === mode) return;
    setMode(next);
    // アクティブな検索があれば新モードで即再実行（結果とモードの不整合を残さない）
    if (query && accountId) {
      void search(accountId, query);
    }
  };

  return (
    <div className="flex gap-1 px-3 pb-1">
      {MODES.map((m) => (
        <button
          key={m.value}
          type="button"
          aria-pressed={mode === m.value}
          onClick={() => handleSelect(m.value)}
          className={`rounded px-2 py-0.5 text-xs ${
            mode === m.value ? "bg-blue-100 text-blue-700 font-semibold" : "text-gray-500 hover:bg-gray-100"
          }`}
        >
          {m.label}
        </button>
      ))}
    </div>
  );
}
```

`Sidebar.tsx`: `<SearchBar onSearch={handleSearch} onClear={handleClearSearch} />` の直後に `<SearchModeToggle accountId={selectedAccountId} />` を追加。スタイリングは周辺（SearchBar）の余白に合わせて微調整してよい。

- [ ] **Step 4: Green を確認**

Run: `pnpm test && pnpm tsc --noEmit`
Expected: 全 PASS（既存 Sidebar.test.tsx が壊れていないこと）

- [ ] **Step 5: コミット**

```bash
git add src/components/sidebar/SearchModeToggle.tsx src/components/sidebar/Sidebar.tsx src/__tests__/SearchModeToggle.test.tsx
git commit -m "feat(ui): 検索バー直下に文字列/ベクトルのモードトグルを追加"
```

**→ ここで PR C を作成**（タイトル例: `feat(search): 文字列検索とセマンティック検索を切り替えるUIを追加`。本文に「デフォルトは文字列・localStorage 永続化・Ollama 未起動時はエラートースト表示（既存エラー経路）」を明記）

---

## Task 3: migration v19 ＋ db::saved_searches CRUD

**Files:**
- Modify: `src-tauri/src/db/migrations.rs`（`migrate_v19` ＋ `MIGRATIONS` 末尾 `(19, migrate_v19),`）
- Create: `src-tauri/src/models/saved_search.rs`（`src-tauri/src/models/mod.rs` に `pub mod saved_search;`）
- Create: `src-tauri/src/db/saved_searches.rs`（`src-tauri/src/db/mod.rs` に `pub mod saved_searches;`）

**Interfaces:**
- Produces（Task 4 が依存）:
  - `models::saved_search::SavedSearch { id: i64, name: String, query: String, mode: String, sort_order: i64, created_at: String }`（Serialize/Deserialize/Clone/Debug）
  - `models::saved_search::CreateSavedSearchRequest { name: String, query: String, mode: String }`（Deserialize/Debug）
  - `db::saved_searches::list_saved_searches(conn) -> Result<Vec<SavedSearch>, AppError>`（ORDER BY sort_order, id）
  - `db::saved_searches::insert_saved_search(conn, req: &CreateSavedSearchRequest) -> Result<SavedSearch, AppError>`（mode が "fulltext"/"semantic" 以外なら CHECK 制約でエラー）
  - `db::saved_searches::rename_saved_search(conn, id: i64, name: &str) -> Result<(), AppError>`
  - `db::saved_searches::delete_saved_search(conn, id: i64) -> Result<(), AppError>`
  - rename/delete の対象なし（0行）時のエラーは **`db/projects.rs` の `update_project` / `delete_project` と同じ AppError の変種・流儀に合わせる**（実装時に必ず読んで揃える）

- [ ] **Step 1: 失敗するテストを書く**

`src-tauri/src/db/saved_searches.rs` にスタブ＋テスト:

```rust
//! saved_searches（スマートビュー＝保存検索）の CRUD。
//! 「検索の保存」であり新しい概念を増やさない（設計書 Phase 3）。
//! 特権メール操作ではないため dispatch バスではなく projects と同じ直 CRUD。

use crate::error::AppError;
use crate::models::saved_search::{CreateSavedSearchRequest, SavedSearch};
use rusqlite::{params, Connection};

pub fn list_saved_searches(conn: &Connection) -> Result<Vec<SavedSearch>, AppError> {
    let _ = conn;
    Ok(Vec::new()) // Step 3 で実装
}

pub fn insert_saved_search(
    conn: &Connection,
    req: &CreateSavedSearchRequest,
) -> Result<SavedSearch, AppError> {
    let _ = (conn, req);
    Err(AppError::Validation("todo".into())) // 変種名は error.rs の実物に合わせる
}

pub fn rename_saved_search(conn: &Connection, id: i64, name: &str) -> Result<(), AppError> {
    let _ = (conn, id, name);
    Ok(())
}

pub fn delete_saved_search(conn: &Connection, id: i64) -> Result<(), AppError> {
    let _ = (conn, id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::setup_db;

    fn req(name: &str, mode: &str) -> CreateSavedSearchRequest {
        CreateSavedSearchRequest {
            name: name.into(),
            query: "照明".into(),
            mode: mode.into(),
        }
    }

    #[test]
    fn test_insert_and_list() {
        let conn = setup_db();
        let s = insert_saved_search(&conn, &req("照明の件", "semantic")).unwrap();
        assert_eq!(s.name, "照明の件");
        assert_eq!(s.mode, "semantic");
        let all = list_saved_searches(&conn).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].query, "照明");
    }

    #[test]
    fn test_invalid_mode_rejected_by_check() {
        let conn = setup_db();
        assert!(insert_saved_search(&conn, &req("x", "hybrid")).is_err());
    }

    #[test]
    fn test_rename() {
        let conn = setup_db();
        let s = insert_saved_search(&conn, &req("旧名", "fulltext")).unwrap();
        rename_saved_search(&conn, s.id, "新名").unwrap();
        assert_eq!(list_saved_searches(&conn).unwrap()[0].name, "新名");
    }

    #[test]
    fn test_rename_missing_is_error() {
        let conn = setup_db();
        assert!(rename_saved_search(&conn, 9999, "x").is_err());
    }

    #[test]
    fn test_delete() {
        let conn = setup_db();
        let s = insert_saved_search(&conn, &req("消す", "fulltext")).unwrap();
        delete_saved_search(&conn, s.id).unwrap();
        assert!(list_saved_searches(&conn).unwrap().is_empty());
        assert!(delete_saved_search(&conn, s.id).is_err(), "二重削除はエラー");
    }

    #[test]
    fn test_list_orders_by_sort_order_then_id() {
        let conn = setup_db();
        let a = insert_saved_search(&conn, &req("a", "fulltext")).unwrap();
        let _b = insert_saved_search(&conn, &req("b", "fulltext")).unwrap();
        conn.execute("UPDATE saved_searches SET sort_order = 10 WHERE id = ?1", [a.id]).unwrap();
        let all = list_saved_searches(&conn).unwrap();
        assert_eq!(all[0].name, "b");
        assert_eq!(all[1].name, "a");
    }
}
```

`src-tauri/src/models/saved_search.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedSearch {
    pub id: i64,
    pub name: String,
    pub query: String,
    pub mode: String,
    pub sort_order: i64,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateSavedSearchRequest {
    pub name: String,
    pub query: String,
    pub mode: String,
}
```

- [ ] **Step 2: Red を確認**

Run: `cd src-tauri && cargo test db::saved_searches`
Expected: FAIL（saved_searches テーブル未作成 / スタブ）

- [ ] **Step 3: migration と CRUD 本体を実装**

`migrations.rs`（設計書の SQL どおり。`MIGRATIONS` 末尾に `(19, migrate_v19),`）:

```rust
/// v19: スマートビュー（保存検索）。クエリとモードをセットで保存する（設計書 Phase 3）。
fn migrate_v19(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS saved_searches (
            id         INTEGER PRIMARY KEY,
            name       TEXT NOT NULL,
            query      TEXT NOT NULL,
            mode       TEXT NOT NULL CHECK (mode IN ('fulltext', 'semantic')),
            sort_order INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        ",
    )?;
    Ok(())
}
```

`db/saved_searches.rs` 本体:

```rust
fn row_to_saved_search(row: &rusqlite::Row) -> rusqlite::Result<SavedSearch> {
    Ok(SavedSearch {
        id: row.get(0)?,
        name: row.get(1)?,
        query: row.get(2)?,
        mode: row.get(3)?,
        sort_order: row.get(4)?,
        created_at: row.get(5)?,
    })
}

const COLS: &str = "id, name, query, mode, sort_order, created_at";

pub fn list_saved_searches(conn: &Connection) -> Result<Vec<SavedSearch>, AppError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM saved_searches ORDER BY sort_order, id"
    ))?;
    let rows = stmt
        .query_map([], row_to_saved_search)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn insert_saved_search(
    conn: &Connection,
    req: &CreateSavedSearchRequest,
) -> Result<SavedSearch, AppError> {
    conn.execute(
        "INSERT INTO saved_searches (name, query, mode) VALUES (?1, ?2, ?3)",
        params![req.name, req.query, req.mode],
    )?;
    let id = conn.last_insert_rowid();
    let s = conn.query_row(
        &format!("SELECT {COLS} FROM saved_searches WHERE id = ?1"),
        [id],
        row_to_saved_search,
    )?;
    Ok(s)
}

// rename_saved_search / delete_saved_search:
//   conn.execute(...) の戻り（変更行数）が 0 のときのエラーは
//   db/projects.rs の update_project / delete_project の流儀（AppError の変種・メッセージ形式）に
//   合わせて実装すること（実物を読んで揃える。独自の新変種を作らない）
```

- [ ] **Step 4: Green を確認**

Run: `cd src-tauri && cargo test db::saved_searches && cargo test`
Expected: 全 PASS（既存 migration テストが v19 で壊れていないこと。合成 migration 番号を使う既存テストがあれば v20 に退避 — v18 のときと同じ対処）

- [ ] **Step 5: コミット**

```bash
git add src-tauri/src/db/migrations.rs src-tauri/src/db/saved_searches.rs src-tauri/src/db/mod.rs src-tauri/src/models/saved_search.rs src-tauri/src/models/mod.rs
git commit -m "feat(db): スマートビュー用のsaved_searchesテーブルとCRUDを追加(v19)"
```

---

## Task 4: commands ＋ API ラッパ ＋ 型

**Files:**
- Create: `src-tauri/src/commands/saved_search_commands.rs`（`commands/mod.rs` に追記）
- Modify: `src-tauri/src/lib.rs`（invoke_handler に4コマンド追加）
- Create: `src/types/savedSearch.ts`
- Create: `src/api/savedSearchApi.ts`

**Interfaces:**
- Consumes: Task 3 の `db::saved_searches::*` / `models::saved_search::*`
- Produces（Task 5〜6 が依存）:
  - commands: `list_saved_searches() -> Vec<SavedSearch>` / `create_saved_search(name, query, mode) -> SavedSearch` / `rename_saved_search(id: i64, name: String)` / `delete_saved_search(id: i64)`（全て `Result<_, String>`、`State<DbState>` + `with_conn` — `project_commands.rs` と同型）
  - `src/types/savedSearch.ts`: `interface SavedSearch { id: number; name: string; query: string; mode: SearchMode; sort_order: number; created_at: string }`
  - `savedSearchApi = { list(): Promise<SavedSearch[]>, create(name, query, mode): Promise<SavedSearch>, rename(id, name): Promise<void>, remove(id): Promise<void> }`（`invokeCommand` 使用、`projectApi.ts` と同型）

- [ ] **Step 1: 失敗するテストを書く**

`saved_search_commands.rs` の `#[cfg(test)]`（`project_commands.rs` のテスト流儀 — commands の本体ロジックは db 層に委譲済みなので、コマンド関数のクエリ部を直接テストするか、`project_commands.rs` がしている粒度に合わせる。最低限:）

```rust
    #[test]
    fn test_create_and_list_roundtrip() {
        let conn = crate::test_helpers::setup_db();
        let created = crate::db::saved_searches::insert_saved_search(
            &conn,
            &crate::models::saved_search::CreateSavedSearchRequest {
                name: "照明".into(),
                query: "灯体".into(),
                mode: "semantic".into(),
            },
        )
        .unwrap();
        let listed = crate::db::saved_searches::list_saved_searches(&conn).unwrap();
        assert_eq!(listed[0].id, created.id);
    }
```

（コマンド関数自体が `State` を要求して単体テストしづらい場合、`project_commands.rs` の既存テストがどう回避しているかを踏襲する）

- [ ] **Step 2: Red → 実装 → Green**

commands 実装（`project_commands.rs` と同型）:

```rust
use crate::db::saved_searches;
use crate::models::saved_search::{CreateSavedSearchRequest, SavedSearch};
use crate::state::DbState;
use tauri::State;

#[tauri::command]
pub fn list_saved_searches(state: State<DbState>) -> Result<Vec<SavedSearch>, String> {
    state
        .with_conn(saved_searches::list_saved_searches)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_saved_search(
    state: State<DbState>,
    name: String,
    query: String,
    mode: String,
) -> Result<SavedSearch, String> {
    state
        .with_conn(|conn| {
            saved_searches::insert_saved_search(conn, &CreateSavedSearchRequest { name, query, mode })
        })
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn rename_saved_search(state: State<DbState>, id: i64, name: String) -> Result<(), String> {
    state
        .with_conn(|conn| saved_searches::rename_saved_search(conn, id, &name))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_saved_search(state: State<DbState>, id: i64) -> Result<(), String> {
    state
        .with_conn(|conn| saved_searches::delete_saved_search(conn, id))
        .map_err(|e| e.to_string())
}
```

（`create_saved_search` のクロージャが `name` 等を move するため `with_conn` のシグネチャと合うことを確認。合わなければ `CreateSavedSearchRequest` をクロージャ外で組み立てる）

`src/types/savedSearch.ts`:

```typescript
import type { SearchMode } from "./search";

export interface SavedSearch {
  id: number;
  name: string;
  query: string;
  mode: SearchMode;
  sort_order: number;
  created_at: string;
}
```

`src/api/savedSearchApi.ts`:

```typescript
import { invokeCommand } from "./client";
import type { SavedSearch } from "../types/savedSearch";
import type { SearchMode } from "../types/search";

export const savedSearchApi = {
  list: () => invokeCommand<SavedSearch[]>("list_saved_searches", {}),
  create: (name: string, query: string, mode: SearchMode) =>
    invokeCommand<SavedSearch>("create_saved_search", { name, query, mode }),
  rename: (id: number, name: string) =>
    invokeCommand<void>("rename_saved_search", { id, name }),
  remove: (id: number) => invokeCommand<void>("delete_saved_search", { id }),
};
```

`lib.rs` の `generate_handler![...]` に4コマンド追加、`commands/mod.rs` に `pub mod saved_search_commands;`。

Run: `cd src-tauri && cargo test && cd .. && pnpm tsc --noEmit`
Expected: 全 PASS

- [ ] **Step 3: コミット**

```bash
git add src-tauri/src/commands/saved_search_commands.rs src-tauri/src/commands/mod.rs src-tauri/src/lib.rs src/types/savedSearch.ts src/api/savedSearchApi.ts
git commit -m "feat(search): 保存検索のTauriコマンドとAPIラッパを追加"
```

---

## Task 5: savedSearchStore

**Files:**
- Create: `src/stores/savedSearchStore.ts`
- Test: `src/__tests__/stores/savedSearchStore.test.ts`

**Interfaces:**
- Consumes: `savedSearchApi`（Task 4）
- Produces（Task 6 が依存）:
  - `useSavedSearchStore`: `{ savedSearches: SavedSearch[], loading: boolean, fetch(): Promise<void>, create(name, query, mode): Promise<void>, rename(id, name): Promise<void>, remove(id): Promise<void> }`
  - create/rename/remove は成功後に `fetch()` で再読込（projects 系ストアの既存流儀に合わせる。エラーは `useErrorStore.getState().addError(...)`）

- [ ] **Step 1: 失敗するテストを書く**（`projectStore.test.ts` の mockInvoke 流儀）

```typescript
import { useSavedSearchStore } from "../../stores/savedSearchStore";

const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

const sample = { id: 1, name: "照明", query: "灯体", mode: "semantic", sort_order: 0, created_at: "2026-07-18" };

describe("savedSearchStore", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useSavedSearchStore.setState({ savedSearches: [], loading: false });
  });

  it("fetch で一覧を取得する", async () => {
    mockInvoke.mockResolvedValue([sample]);
    await useSavedSearchStore.getState().fetch();
    expect(mockInvoke).toHaveBeenCalledWith("list_saved_searches", {});
    expect(useSavedSearchStore.getState().savedSearches).toEqual([sample]);
  });

  it("create は作成後に再取得する", async () => {
    mockInvoke.mockImplementation((cmd: string) =>
      cmd === "list_saved_searches" ? Promise.resolve([sample]) : Promise.resolve(sample)
    );
    await useSavedSearchStore.getState().create("照明", "灯体", "semantic");
    expect(mockInvoke).toHaveBeenCalledWith("create_saved_search", { name: "照明", query: "灯体", mode: "semantic" });
    expect(useSavedSearchStore.getState().savedSearches).toEqual([sample]);
  });

  it("remove は削除後に再取得する", async () => {
    mockInvoke.mockImplementation((cmd: string) =>
      cmd === "list_saved_searches" ? Promise.resolve([]) : Promise.resolve(null)
    );
    await useSavedSearchStore.getState().remove(1);
    expect(mockInvoke).toHaveBeenCalledWith("delete_saved_search", { id: 1 });
  });

  it("rename は改名後に再取得する", async () => {
    mockInvoke.mockImplementation((cmd: string) =>
      cmd === "list_saved_searches" ? Promise.resolve([{ ...sample, name: "新名" }]) : Promise.resolve(null)
    );
    await useSavedSearchStore.getState().rename(1, "新名");
    expect(mockInvoke).toHaveBeenCalledWith("rename_saved_search", { id: 1, name: "新名" });
    expect(useSavedSearchStore.getState().savedSearches[0].name).toBe("新名");
  });
});
```

- [ ] **Step 2: Red → 実装 → Green**

```typescript
import { create } from "zustand";
import { savedSearchApi } from "../api/savedSearchApi";
import type { SavedSearch } from "../types/savedSearch";
import type { SearchMode } from "../types/search";
import { useErrorStore } from "./errorStore"; // 実ファイル名・エラーヘルパは searchStore の流儀に合わせる

interface SavedSearchState {
  savedSearches: SavedSearch[];
  loading: boolean;
  fetch: () => Promise<void>;
  create: (name: string, query: string, mode: SearchMode) => Promise<void>;
  rename: (id: number, name: string) => Promise<void>;
  remove: (id: number) => Promise<void>;
}

export const useSavedSearchStore = create<SavedSearchState>((set, get) => ({
  savedSearches: [],
  loading: false,
  fetch: async () => {
    set({ loading: true });
    try {
      set({ savedSearches: await savedSearchApi.list() });
    } catch (e) {
      useErrorStore.getState().addError(String(e));
    } finally {
      set({ loading: false });
    }
  },
  create: async (name, query, mode) => {
    try {
      await savedSearchApi.create(name, query, mode);
      await get().fetch();
    } catch (e) {
      useErrorStore.getState().addError(String(e));
    }
  },
  rename: async (id, name) => {
    try {
      await savedSearchApi.rename(id, name);
      await get().fetch();
    } catch (e) {
      useErrorStore.getState().addError(String(e));
    }
  },
  remove: async (id) => {
    try {
      await savedSearchApi.remove(id);
      await get().fetch();
    } catch (e) {
      useErrorStore.getState().addError(String(e));
    }
  },
}));
```

（エラー整形は searchStore が使う `errorMessage(e)` ヘルパがあればそれに合わせる）

Run: `pnpm test savedSearchStore && pnpm tsc --noEmit`
Expected: PASS

- [ ] **Step 3: コミット**

```bash
git add src/stores/savedSearchStore.ts src/__tests__/stores/savedSearchStore.test.ts
git commit -m "feat(search): 保存検索のZustandストアを追加"
```

---

## Task 6: スマートビュー UI（サイドバー・保存ボタン・並び替え）

**Files:**
- Create: `src/components/sidebar/SmartViewList.tsx`
- Modify: `src/components/sidebar/Sidebar.tsx`（スクロール領域内・ProjectTree の後に配置）
- Modify: `src/components/thread-list/SearchResults.tsx`（「この検索を保存」＋ 関連度/日付 並び替えトグル）
- Test: `src/__tests__/SmartViewList.test.tsx`、`src/__tests__/SearchResults.test.tsx`（既存があれば追記）

**Interfaces:**
- Consumes: `useSavedSearchStore`（Task 5）、`useSearchStore`（mode/setMode/search/query）、`useUiStore.setViewMode`、`ContextMenu`（`src/components/common/ContextMenu.tsx`: props `{ x, y, items: {label, onClick, danger?}[], onClose }`）
- Produces: `SmartViewList`（props: `accountId: string | null`）

**挙動仕様:**
- SmartViewList: セクション見出し「スマートビュー」（ProjectTree の見出しと同じ `text-xs font-semibold uppercase ...` パターン）。行クリックで `setMode(saved.mode)` → `search(accountId, saved.query)` → `setViewMode("search")`。右クリックで ContextMenu（`名前変更`＝インライン入力 or window.prompt 相当の簡易実装、`削除`＝danger）。マウント時 `fetch()`
- SearchResults: 結果ヘッダに「この検索を保存」ボタン（クリックでインライン名前入力→ `create(name, query, mode)`）。並び替えトグル `関連度順`（=バックエンド到着順のまま）/ `日付順`（=クライアント側で `mail.date` 降順ソート）。ソートは表示のみに作用し `selectedIndex` の対象配列と一致させること（ソート済み配列を単一のソースにする）

- [ ] **Step 1: 失敗するテストを書く**

`SmartViewList.test.tsx`:

```tsx
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { SmartViewList } from "../components/sidebar/SmartViewList";
import { useSavedSearchStore } from "../stores/savedSearchStore";
import { useSearchStore } from "../stores/searchStore";

const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

const saved = { id: 1, name: "照明", query: "灯体", mode: "semantic" as const, sort_order: 0, created_at: "2026-07-18" };

describe("SmartViewList", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockInvoke.mockResolvedValue([]);
    useSavedSearchStore.setState({ savedSearches: [saved], loading: false });
    useSearchStore.setState({ mode: "fulltext", query: "", results: [], searching: false, selectedIndex: -1 });
  });

  it("保存済み検索を一覧表示する", () => {
    render(<SmartViewList accountId="acc1" />);
    expect(screen.getByText("スマートビュー")).toBeInTheDocument();
    expect(screen.getByText("照明")).toBeInTheDocument();
  });

  it("クリックで保存されたモード・クエリで検索を実行する", async () => {
    render(<SmartViewList accountId="acc1" />);
    fireEvent.click(screen.getByText("照明"));
    await waitFor(() => {
      expect(useSearchStore.getState().mode).toBe("semantic");
      expect(mockInvoke).toHaveBeenCalledWith("semantic_search", { accountId: "acc1", query: "灯体" });
    });
  });

  it("右クリックメニューから削除できる", async () => {
    render(<SmartViewList accountId="acc1" />);
    fireEvent.contextMenu(screen.getByText("照明"));
    fireEvent.click(screen.getByText("削除"));
    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("delete_saved_search", { id: 1 });
    });
  });
});
```

`SearchResults` への追記テスト（保存ボタン。既存テストファイルの mock 構成に合わせて追記）:

```tsx
  it("この検索を保存 で名前を付けて保存できる", async () => {
    useSearchStore.setState({ query: "灯体", mode: "semantic", results: [/* 既存テストのフィクスチャ1件 */], searching: false, selectedIndex: -1 });
    render(<SearchResults />);
    fireEvent.click(screen.getByRole("button", { name: "この検索を保存" }));
    fireEvent.change(screen.getByPlaceholderText("ビュー名"), { target: { value: "照明" } });
    fireEvent.keyDown(screen.getByPlaceholderText("ビュー名"), { key: "Enter" });
    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("create_saved_search", { name: "照明", query: "灯体", mode: "semantic" });
    });
  });
```

- [ ] **Step 2: Red を確認**

Run: `pnpm test SmartViewList SearchResults`
Expected: FAIL

- [ ] **Step 3: 実装**

`SmartViewList.tsx`（ProjectTree の構造・スタイルを踏襲。約80行想定）:

```tsx
import { useEffect, useState } from "react";
import { ContextMenu } from "../common/ContextMenu";
import { useSavedSearchStore } from "../../stores/savedSearchStore";
import { useSearchStore } from "../../stores/searchStore";
import { useUiStore } from "../../stores/uiStore"; // setViewMode の実所在に合わせる

interface SmartViewListProps {
  accountId: string | null;
}

export function SmartViewList({ accountId }: SmartViewListProps) {
  const { savedSearches, fetch, rename, remove } = useSavedSearchStore();
  const setMode = useSearchStore((s) => s.setMode);
  const search = useSearchStore((s) => s.search);
  const setViewMode = useUiStore((s) => s.setViewMode);
  const [menu, setMenu] = useState<{ x: number; y: number; id: number } | null>(null);
  const [renaming, setRenaming] = useState<{ id: number; value: string } | null>(null);

  useEffect(() => {
    void fetch();
  }, [fetch]);

  if (savedSearches.length === 0) return null;

  const run = (id: number) => {
    const s = savedSearches.find((v) => v.id === id);
    if (!s || !accountId) return;
    setMode(s.mode);
    void search(accountId, s.query);
    setViewMode("search");
  };

  return (
    <div className="mt-2">
      <div className="px-3 py-1">
        <span className="text-xs font-semibold uppercase tracking-wide text-gray-400">
          スマートビュー
        </span>
      </div>
      <ul>
        {savedSearches.map((s) => (
          <li key={s.id}>
            {renaming?.id === s.id ? (
              <input
                autoFocus
                className="mx-3 w-11/12 rounded border px-1 text-sm"
                value={renaming.value}
                onChange={(e) => setRenaming({ id: s.id, value: e.target.value })}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && renaming.value.trim()) {
                    void rename(s.id, renaming.value.trim());
                    setRenaming(null);
                  }
                  if (e.key === "Escape") setRenaming(null);
                }}
              />
            ) : (
              <button
                type="button"
                className="w-full px-3 py-1 text-left text-sm hover:bg-gray-100"
                onClick={() => run(s.id)}
                onContextMenu={(e) => {
                  e.preventDefault();
                  setMenu({ x: e.clientX, y: e.clientY, id: s.id });
                }}
              >
                🔎 {s.name}
              </button>
            )}
          </li>
        ))}
      </ul>
      {menu && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          onClose={() => setMenu(null)}
          items={[
            {
              label: "名前変更",
              onClick: () => {
                const s = savedSearches.find((v) => v.id === menu.id);
                if (s) setRenaming({ id: s.id, value: s.name });
              },
            },
            { label: "削除", danger: true, onClick: () => void remove(menu.id) },
          ]}
        />
      )}
    </div>
  );
}
```

`Sidebar.tsx`: スクロール領域（`flex-1 overflow-y-auto` の div）内、ProjectTree の後に `<SmartViewList accountId={selectedAccountId} />`。

`SearchResults.tsx` への追加（構造は既存に合わせ、ヘッダ行に2要素追加）:

```tsx
// 追加 state:
//   const [saving, setSaving] = useState(false);
//   const [saveName, setSaveName] = useState("");
//   const [sortBy, setSortBy] = useState<"relevance" | "date">("relevance");
// 追加 store:
//   const mode = useSearchStore((s) => s.mode);
//   const createSaved = useSavedSearchStore((s) => s.create);
// 表示配列（selectedIndex との整合のため、この配列を map と useEffect の両方で使う）:
//   const displayResults = sortBy === "date"
//     ? [...results].sort((a, b) => b.mail.date.localeCompare(a.mail.date))
//     : results;
// ヘッダに:
//   <button type="button" onClick={() => setSaving(true)}>この検索を保存</button>
//   {saving && (
//     <input placeholder="ビュー名" value={saveName} autoFocus
//       onChange={(e) => setSaveName(e.target.value)}
//       onKeyDown={(e) => {
//         if (e.key === "Enter" && saveName.trim()) {
//           void createSaved(saveName.trim(), query, mode);
//           setSaving(false); setSaveName("");
//         }
//         if (e.key === "Escape") { setSaving(false); setSaveName(""); }
//       }} />
//   )}
//   並び替え: <button aria-pressed={sortBy==="relevance"} onClick={() => setSortBy("relevance")}>関連度順</button>
//            <button aria-pressed={sortBy==="date"} onClick={() => setSortBy("date")}>日付順</button>
```

（`selectedIndex` → 右ペイン同期の `useEffect` は `displayResults[selectedIndex]` を参照するよう変更。日付順は `mail.date` の文字列降順 — ISO 形式なので localeCompare で正しく並ぶ）

- [ ] **Step 4: Green を確認**

Run: `pnpm test && pnpm tsc --noEmit`
Expected: 全 PASS（既存 Sidebar / SearchResults テストの回帰なし）

- [ ] **Step 5: コミット（2コミット）**

```bash
git add src/components/sidebar/SmartViewList.tsx src/components/sidebar/Sidebar.tsx src/__tests__/SmartViewList.test.tsx
git commit -m "feat(ui): サイドバーにスマートビュー(保存検索)セクションを追加"
git add src/components/thread-list/SearchResults.tsx src/__tests__/SearchResults.test.tsx
git commit -m "feat(ui): 検索結果に保存ボタンと関連度/日付の並び替えを追加"
```

---

## Task 7: 仕上げ（lint・全テスト・実機確認・PR）

- [ ] **Step 1: lint と整形**

Run: `cd src-tauri && cargo fmt --check && cargo clippy --all-targets 2>&1 | tail -20`
Expected: 触ったファイルにエラー・警告なし（既存負債は範囲外）

- [ ] **Step 2: 全テスト**

Run: `cd src-tauri && cargo test && cd .. && pnpm test && pnpm tsc --noEmit`
Expected: 全 PASS

- [ ] **Step 3: 実機確認（デバッグビルド）**

`pnpm tauri build --debug` → 起動して:
1. トグルで「ベクトル」を選び検索 → セマンティック結果が出る（Ollama+bge-m3 前提。未起動ならエラートーストが出て文字列モードは無傷なこと）
2. 「この検索を保存」→ サイドバーに現れる → クリックで再実行（モードも復元）
3. 右クリック → 名前変更・削除
4. アプリ再起動 → モードとスマートビューが維持されている（モード=localStorage、ビュー=DB）
5. 並び替えトグルで日付順⇄関連度順が切り替わり、j/k 選択と右ペイン表示が表示順と一致する

- [ ] **Step 4: PR 作成**

PR C（未作成ならここで）と PR D を作成。PR D タイトル例: `feat(search): 検索を保存してサイドバーから再実行できるスマートビューを追加`。Stacked（base = PR C）を明記。

---

## 実装順序とレビュー観点（コントローラ向けメモ）

- Task 1 の `search()` 呼び分けは既存エラー経路・`searching` フラグの挙動を変えないこと（レビューで既存テストの回帰を確認）
- Task 6 が最も裁量が大きい。レビュー観点: 並び替えが `selectedIndex` 系（j/k・右ペイン同期）と単一の表示配列を共有しているか / ContextMenu・見出しが ProjectTree の既存パターンと一致しているか
- saved_searches は account 非スコープ（設計書の SQL どおり）。実行時は現在選択中のアカウントで検索する。複数アカウントでの共有は既知の仕様
- sort_order のドラッグ並べ替え UI はスコープ外（YAGNI。カラムと ORDER BY だけ用意）

## 次の候補（本計画完了後）

設計書の将来拡張: ハイブリッド検索（FTS＋ベクトル融合の第3モード）、スマートビューの未読バッジ・案件スコープ絞り込み、埋め込みモデル差し替え（vec_chunks 再作成＋poison チャンク退避）
