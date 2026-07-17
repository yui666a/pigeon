# SecureStore enum 化によるテスト高速化 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `SecureStore` を enum 化してテストを InMemory 実装に切り替え、実 Stronghold 必須テストを `#[ignore]` 化することで PR CI の Rust テストを 16分 → 3〜4分に短縮する。

**Architecture:** 現行 `SecureStore`（Stronghold 実装）を `StrongholdStore` にリネームし、`SecureStore` を `enum { Stronghold(StrongholdStore), InMemory(Mutex<HashMap<String, Vec<u8>>>) }` として再定義する。本番コンストラクタ（`new` / `open_with_migration`）は `Stronghold` バリアントを返し、呼び出し側 50 箱所超のシグネチャは不変。テストは `SecureStore::in_memory()` に切替。移行系 6 テストは `#[ignore]` 化し日次 cron で担保する。

**Tech Stack:** Rust, iota_stronghold 2.1, Tauri 2, cargo test, GitHub Actions

## Global Constraints

- `unwrap()` / `expect()` はテストコード以外で使用しない（agent.md）
- アプリケーションエラーは `AppError`（thiserror）で定義する
- `cargo fmt` はリポジトリ全体でなく変更ファイルのみ整形する意識で行い、最後に `cargo fmt` 差分を確認する
- マイグレーション番号の変更は本計画には無い（DB スキーマ非変更）
- 秘密情報を平文でログ出力しない（ADR 0003 / CLAUDE.md セキュリティルール）

---

### Task 1: SecureStore を enum 化し InMemory バリアントを追加する

**Files:**
- Modify: `src-tauri/src/secure_store.rs`（現行 `struct SecureStore` = 270-416 行付近、テストモジュール = 418 行以降）

**Interfaces:**
- Consumes: なし（既存 `iota_stronghold`、`AppError`、`Mutex`、`HashMap`）
- Produces:
  - `pub enum SecureStore { Stronghold(StrongholdStore), InMemory(std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>) }`
  - `pub struct StrongholdStore { inner: Mutex<SecureStoreInner> }`（現行 `SecureStore` の中身を移設）
  - `impl SecureStore` のメソッド（シグネチャ不変）:
    - `pub fn new(path: PathBuf, password: &[u8]) -> Result<Self, AppError>`
    - `pub fn open_with_migration(path: PathBuf, key: &[u8]) -> Result<(Self, MasterKeyMigration), AppError>`
    - `pub fn insert(&self, key: &str, value: &[u8]) -> Result<(), AppError>`
    - `pub fn get(&self, key: &str) -> Result<Option<Vec<u8>>, AppError>`
    - `pub fn delete(&self, key: &str) -> Result<(), AppError>`
    - `pub fn in_memory() -> Self`（新設・テスト/フォールバック用途）

- [ ] **Step 1: InMemory の契約を確認する失敗テストを追加する**

`src-tauri/src/secure_store.rs` のテストモジュール（`#[cfg(test)] mod tests { use super::*; ... }`）の先頭付近に追加:

```rust
#[test]
fn test_in_memory_insert_get_roundtrip() {
    let store = SecureStore::in_memory();
    store.insert("k", b"v").unwrap();
    assert_eq!(store.get("k").unwrap().as_deref(), Some(b"v".as_ref()));
}

#[test]
fn test_in_memory_get_missing_returns_none() {
    let store = SecureStore::in_memory();
    assert_eq!(store.get("nope").unwrap(), None);
}

#[test]
fn test_in_memory_overwrite_and_delete() {
    let store = SecureStore::in_memory();
    store.insert("k", b"v1").unwrap();
    store.insert("k", b"v2").unwrap();
    assert_eq!(store.get("k").unwrap().as_deref(), Some(b"v2".as_ref()));
    store.delete("k").unwrap();
    assert_eq!(store.get("k").unwrap(), None);
}
```

- [ ] **Step 2: テストが失敗（コンパイルエラー）することを確認**

Run: `cd src-tauri && cargo test in_memory 2>&1 | tail -20`
Expected: FAIL — `no function or associated item named 'in_memory' found for enum/struct SecureStore`

- [ ] **Step 3: 現行 SecureStore を StrongholdStore にリネームし enum を導入する**

`src-tauri/src/secure_store.rs` の現行定義を次のように変更する。

現行:
```rust
pub struct SecureStore {
    inner: Mutex<SecureStoreInner>,
}
```
を:
```rust
pub struct StrongholdStore {
    inner: Mutex<SecureStoreInner>,
}

/// 秘密情報の保管先。本番は Stronghold、テストは InMemory。
///
/// enum ディスパッチにより呼び出し側の `&SecureStore` を変えずに
/// テストで実 Stronghold（スナップショット I/O が 1 回 55 秒）を回避する。
pub enum SecureStore {
    Stronghold(StrongholdStore),
    InMemory(Mutex<std::collections::HashMap<String, Vec<u8>>>),
}
```

続いて現行 `impl SecureStore { pub fn new ... pub fn open_with_migration ... }` を `impl StrongholdStore` に付け替える。ただし `new` / `open_with_migration` は enum を返す必要があるため、以下の 2 段構成にする:

`impl StrongholdStore` 側（現行ロジックをそのまま移設。戻り値の `Self`/`Ok(Self { ... })` は `StrongholdStore` を指す）:
```rust
impl StrongholdStore {
    fn new(path: PathBuf, password: &[u8]) -> Result<Self, AppError> {
        // 現行 SecureStore::new の本体をそのまま移設
        // (snapshot_path, stronghold, keyprovider, load_snapshot, client_path, load/create client)
        // 末尾は Ok(Self { inner: Mutex::new(SecureStoreInner { ... }) })
    }

    fn open_with_migration(
        path: PathBuf,
        key: &[u8],
    ) -> Result<(Self, MasterKeyMigration), AppError> {
        // 現行 SecureStore::open_with_migration の本体をそのまま移設
        // 内部の Self::new は StrongholdStore::new を指す
    }

    fn reencrypt(&self, new_password: &[u8]) -> Result<(), AppError> {
        // 現行のまま移設
    }

    fn insert(&self, key: &str, value: &[u8]) -> Result<(), AppError> {
        // 現行のまま移設
    }
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, AppError> {
        // 現行のまま移設
    }
    fn delete(&self, key: &str) -> Result<(), AppError> {
        // 現行のまま移設
    }
}
```

新規 `impl SecureStore`（enum ラッパ。公開 API を維持）:
```rust
impl SecureStore {
    pub fn new(path: PathBuf, password: &[u8]) -> Result<Self, AppError> {
        Ok(SecureStore::Stronghold(StrongholdStore::new(path, password)?))
    }

    pub fn open_with_migration(
        path: PathBuf,
        key: &[u8],
    ) -> Result<(Self, MasterKeyMigration), AppError> {
        let (store, migration) = StrongholdStore::open_with_migration(path, key)?;
        Ok((SecureStore::Stronghold(store), migration))
    }

    /// テスト/フォールバック用のインメモリ実装。スナップショット I/O を行わない。
    pub fn in_memory() -> Self {
        SecureStore::InMemory(Mutex::new(std::collections::HashMap::new()))
    }

    pub fn insert(&self, key: &str, value: &[u8]) -> Result<(), AppError> {
        match self {
            SecureStore::Stronghold(s) => s.insert(key, value),
            SecureStore::InMemory(m) => {
                let mut map = m.lock().map_err(|e| AppError::Stronghold(e.to_string()))?;
                map.insert(key.to_string(), value.to_vec());
                Ok(())
            }
        }
    }

    pub fn get(&self, key: &str) -> Result<Option<Vec<u8>>, AppError> {
        match self {
            SecureStore::Stronghold(s) => s.get(key),
            SecureStore::InMemory(m) => {
                let map = m.lock().map_err(|e| AppError::Stronghold(e.to_string()))?;
                Ok(map.get(key).cloned())
            }
        }
    }

    pub fn delete(&self, key: &str) -> Result<(), AppError> {
        match self {
            SecureStore::Stronghold(s) => s.delete(key),
            SecureStore::InMemory(m) => {
                let mut map = m.lock().map_err(|e| AppError::Stronghold(e.to_string()))?;
                map.remove(key);
                Ok(())
            }
        }
    }
}
```

注意:
- `MasterKeyMigration` enum・`legacy_fixed_key()`・`SecureStoreInner` struct はそのまま維持する。
- `impl StrongholdStore` の `new` / `open_with_migration` / `reencrypt` は非 pub でよい（同一モジュール内の `SecureStore` から呼ぶため）。ただし `secure_store.rs` のテスト（Task 3 で `#[ignore]` 化）が `StrongholdStore::new` を直接使うなら `pub(crate)` にする。→ Task 3 のテストは `SecureStore::new` 経由に統一するため非 pub のままで良い。

- [ ] **Step 4: テストが通ることを確認**

Run: `cd src-tauri && cargo test in_memory 2>&1 | tail -20`
Expected: PASS（3 テスト green）。実 Stronghold を叩かないため一瞬で終わる。

- [ ] **Step 5: 全体コンパイルが通ることを確認**

Run: `cd src-tauri && cargo build 2>&1 | tail -20`
Expected: エラーなし（呼び出し側 50 箱所は `&SecureStore` のまま変更不要）。

- [ ] **Step 6: fmt / clippy**

Run: `cd src-tauri && cargo fmt && cargo clippy --lib 2>&1 | tail -20`
Expected: 警告・エラーなし

- [ ] **Step 7: コミット**

```bash
git add src-tauri/src/secure_store.rs
git commit -m "refactor(db): SecureStoreをenum化しInMemoryバリアントを追加

Stronghold実装をStrongholdStoreに移設し、SecureStoreをenum化。
呼び出し側のシグネチャは不変。テストがStrongholdのスナップショット
I/O(1回55秒)を回避できるようになる。

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014BMfTpp9YDayfx35jYWsU7"
```

---

### Task 2: 他5ファイルのテスト setup を in_memory() に切り替える

**Files:**
- Modify: `src-tauri/src/classifier/factory.rs:172-179`（`setup()`）
- Modify: `src-tauri/src/commands/settings_commands.rs:181-190`（`setup()`）
- Modify: `src-tauri/src/commands/account_commands.rs:99, 123`（2 箱所）
- Modify: `src-tauri/src/usecase/cases/flag.rs:140-145`
- Modify: `src-tauri/src/usecase/cases/mailbox.rs:285-290`

**Interfaces:**
- Consumes: `SecureStore::in_memory()`（Task 1）
- Produces: なし（テストヘルパの内部変更のみ）

- [ ] **Step 1: settings_commands.rs の setup を切り替える**

`src-tauri/src/commands/settings_commands.rs` の `setup()`:
```rust
    fn setup() -> (Connection, SecureStore, TempDir) {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        let dir = TempDir::new().unwrap();
        // SecureStore/Stronghold expects a fixed-size (32-byte) key, so hash the
        // test password the same way lib.rs derives the real key (see lib.rs).
        let key = sha2::Sha256::digest(b"pw-123456");
        let store = SecureStore::new(dir.path().join("t.stronghold"), &key).unwrap();
        (conn, store, dir)
    }
```
を:
```rust
    fn setup() -> (Connection, SecureStore, TempDir) {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        // InMemory SecureStore は実 Stronghold のスナップショット I/O(1回55秒)を
        // 回避する。TempDir は戻り値の互換のため残すが未使用。
        let dir = TempDir::new().unwrap();
        let store = SecureStore::in_memory();
        (conn, store, dir)
    }
```

注: 戻り値タプル `(Connection, SecureStore, TempDir)` は呼び出し側テストの分解パターンを壊さないため維持する。`TempDir` を消すと各テストの `let (conn, store, _dir) = setup();` が壊れる可能性があるため、まず現状の分解パターンを確認してから消すか判断する。安全側で TempDir を残す。

- [ ] **Step 2: factory.rs の setup を切り替える**

`src-tauri/src/classifier/factory.rs:172-179` を確認し、同様に `SecureStore::new(dir.path().join("test.stronghold"), &key)` を `SecureStore::in_memory()` に置換。`key`（sha256 ダイジェスト）が他で未使用になるなら削除。`dir` は戻り値/他用途を確認して残すか `_` にする。

置換前後の該当行のみ:
```rust
        // 置換前
        let store = SecureStore::new(dir.path().join("test.stronghold"), &key).unwrap();
        // 置換後
        let store = SecureStore::in_memory();
```
`key` がこの store 生成のためだけに存在するなら、その `let key = ...;` 行も削除する。

- [ ] **Step 3: account_commands.rs の2箱所を切り替える**

`src-tauri/src/commands/account_commands.rs:99, 123` の各:
```rust
        let store = crate::secure_store::SecureStore::new(dir.path().join("test.stronghold"), &key)
            .unwrap();
```
を:
```rust
        let store = crate::secure_store::SecureStore::in_memory();
```
に置換。周辺で `key` / `dir` が他に使われていなければ該当行を削除する。

- [ ] **Step 4: flag.rs / mailbox.rs を切り替える**

`src-tauri/src/usecase/cases/flag.rs:143` と `src-tauri/src/usecase/cases/mailbox.rs:288` の:
```rust
        crate::secure_store::SecureStore::new(dir.path().join("test.stronghold"), &TEST_KEY)
```
を:
```rust
        crate::secure_store::SecureStore::in_memory()
```
に置換（`.unwrap()` が付いている場合、`in_memory()` は `Result` を返さないので `.unwrap()` を除去する）。`TEST_KEY` / `dir` がこの箇所専用なら削除。

- [ ] **Step 5: 変更した各ファイルのテストが通ることを確認**

Run:
```bash
cd src-tauri && cargo test --lib classifier::factory 2>&1 | tail -5
cargo test --lib commands::settings_commands 2>&1 | tail -5
cargo test --lib commands::account_commands 2>&1 | tail -5
cargo test --lib usecase::cases::flag 2>&1 | tail -5
cargo test --lib usecase::cases::mailbox 2>&1 | tail -5
```
Expected: 各 PASS。各ファイル数秒以内で完了（従来は分単位だった）。

- [ ] **Step 6: 未使用変数/インポートの警告を解消**

Run: `cd src-tauri && cargo build 2>&1 | grep -E "warning|unused" | head`
Expected: `unused variable: key` 等が出たら該当行（`let key`/`let dir`/`use tempfile::TempDir` 等）を削除して解消。TempDir を戻り値に含めているファイルでは `dir` は残す。再度 build して警告ゼロを確認。

- [ ] **Step 7: fmt & コミット**

```bash
cd src-tauri && cargo fmt
git add src-tauri/src/classifier/factory.rs src-tauri/src/commands/settings_commands.rs src-tauri/src/commands/account_commands.rs src-tauri/src/usecase/cases/flag.rs src-tauri/src/usecase/cases/mailbox.rs
git commit -m "test(db): SecureStoreを使うテストをin_memory()に切り替え

実Strongholdを叩いていた5ファイルのテストsetupをInMemoryに変更。
スナップショットI/O(1回55秒)を回避し、テスト実行を大幅短縮する。

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014BMfTpp9YDayfx35jYWsU7"
```

---

### Task 3: secure_store.rs の実 Stronghold 必須テスト6件を #[ignore] 化する

**Files:**
- Modify: `src-tauri/src/secure_store.rs`（テストモジュール内の 6 テスト）

**Interfaces:**
- Consumes: なし
- Produces: なし（テスト属性の付与のみ）

- [ ] **Step 1: 6テストに #[ignore] を付与する**

以下の 6 テストそれぞれの `#[test]` の直後（または直前）に `#[ignore = "..."]` を追加する。対象:
- `test_secure_store_reopen_reads_persisted_value`
- `test_open_with_migration_fresh_store`
- `test_open_with_migration_reopens_with_current_key`
- `test_open_with_migration_migrates_legacy_snapshot`
- `test_open_with_migration_unreadable_snapshot_backed_up`
- `test_stronghold_snapshots_not_cross_decryptable`

各テストを次の形にする（例）:
```rust
    #[test]
    #[ignore = "実StrongholdのスナップショットI/Oが1回55秒。日次nightly-strongholdジョブで担保"]
    fn test_open_with_migration_migrates_legacy_snapshot() {
        // 本体は変更しない
    }
```

- [ ] **Step 2: 通常の cargo test で 6件がスキップされることを確認**

Run: `cd src-tauri && cargo test --lib secure_store 2>&1 | tail -10`
Expected: `secure_store::tests` の実行が数秒で終わり、結果に `6 ignored`（他の secure_store の高速テストは pass）が含まれる。60 秒超のテストが 1 件も出ないこと。

- [ ] **Step 3: --ignored で 6件が従来通りパスすることを確認**

Run: `cd src-tauri && cargo test --lib secure_store -- --ignored 2>&1 | tail -10`
Expected: 6 テストが PASS（時間はかかる。数分）。ロジックの回帰がないことの確認。

- [ ] **Step 4: リポジトリ全体テストの実行時間を実測する**

Run: `cd src-tauri && time cargo test 2>&1 | tail -5`
Expected: 全テスト PASS、かつ実行時間（コンパイル済み前提のテスト実行部分）が数秒〜十数秒に短縮されていること。`X ignored` に 6 が含まれる。

- [ ] **Step 5: コミット**

```bash
git add src-tauri/src/secure_store.rs
git commit -m "test(db): 実Stronghold必須の移行テスト6件を#[ignore]化

鍵移行・相互復号不可のStronghold固有テストはスナップショットI/Oが重く
PR CIを16分に押し上げていた。#[ignore]でCI本線から外し、--ignoredで
ローカル/日次実行できるようにする。

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014BMfTpp9YDayfx35jYWsU7"
```

---

### Task 4: 日次 nightly-stronghold CI ジョブを追加する

**Files:**
- Create: `.github/workflows/nightly-stronghold.yml`
- Reference: `.github/workflows/test.yml`（依存関係・toolchain・cache のセットアップを踏襲）

**Interfaces:**
- Consumes: `#[ignore]` 付きテスト（Task 3）
- Produces: なし

- [ ] **Step 1: ワークフローファイルを作成する**

`test.yml` の `rust-test` ジョブのセットアップ（checkout、apt 依存、rust-toolchain、rust-cache）を踏襲し、テスト実行を `cargo test -- --ignored` にする。`.github/workflows/nightly-stronghold.yml`:

```yaml
name: Nightly Stronghold Tests

# 実 Stronghold を叩く受け入れテスト（スナップショット I/O が重く PR CI から
# 外している #[ignore] 付きテスト）を日次で実行し、鍵移行・相互復号不可の
# 回帰を担保する。手動実行も可能。
on:
  schedule:
    - cron: "0 18 * * *" # 毎日 03:00 JST (18:00 UTC)
  workflow_dispatch:

permissions:
  contents: read

jobs:
  stronghold-test:
    name: Stronghold Acceptance Tests
    runs-on: ubuntu-latest
    defaults:
      run:
        working-directory: src-tauri
    steps:
      - uses: actions/checkout@34e114876b0b11c390a56381ad16ebd13914f8d5 # v4

      - name: Install system dependencies
        run: |
          sudo apt-get update
          sudo apt-get install -y libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf libssl-dev libglib2.0-dev libgtk-3-dev

      - uses: dtolnay/rust-toolchain@4be7066ada62dd38de10e7b70166bc74ed198c30 # stable

      - uses: Swatinem/rust-cache@e18b497796c12c097a38f9edb9d0641fb99eee32 # v2
        with:
          workspaces: src-tauri

      - name: Run ignored (Stronghold) tests
        run: cargo test -- --ignored
```

注: サードパーティ action のフル SHA ピンは `test.yml` の現行値をそのままコピーする（上記 SHA は 2026-07-17 時点の test.yml の値。作成時に test.yml を再確認して一致させる）。

- [ ] **Step 2: YAML の構文を検証する**

Run: `cd /Users/h.aiso/Projects/pigeon && python3 -c "import yaml; yaml.safe_load(open('.github/workflows/nightly-stronghold.yml'))" && echo OK`
Expected: `OK`

- [ ] **Step 3: test.yml の action SHA と一致しているか確認する**

Run: `cd /Users/h.aiso/Projects/pigeon && grep -E "uses:" .github/workflows/test.yml .github/workflows/nightly-stronghold.yml`
Expected: checkout / rust-toolchain / rust-cache の SHA が両ファイルで一致していること。ズレていれば nightly 側を test.yml に合わせる。

- [ ] **Step 4: コミット**

```bash
git add .github/workflows/nightly-stronghold.yml
git commit -m "ci(db): 実Strongholdテストを日次実行するnightly-strongholdを追加

#[ignore]化した受け入れテスト(鍵移行・相互復号不可)をcronで日次実行し
カバレッジを担保する。workflow_dispatchで手動実行も可能。

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_014BMfTpp9YDayfx35jYWsU7"
```

---

## 検証（全タスク完了後）

- [ ] `cd src-tauri && cargo test` が全 PASS かつ `6 ignored`、テスト実行部分が数秒〜十数秒
- [ ] `cd src-tauri && cargo test -- --ignored` が 6 件 PASS
- [ ] `cargo clippy --all-targets 2>&1 | tail` で警告なし
- [ ] `cargo fmt --check` 差分なし
- [ ] PR 作成後、CI（test.yml）の Rust Tests ジョブが 3〜4 分程度で完了することを確認
