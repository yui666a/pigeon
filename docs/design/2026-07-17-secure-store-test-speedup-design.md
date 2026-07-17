# SecureStore の enum 化による Rust テスト高速化 設計書

作成日: 2026-07-17
ステータス: レビュー待ち

## 背景と課題

PR の CI（`.github/workflows/test.yml` の `rust-test` ジョブ）で `cargo test` の実行に **約16分**かかっており、PR のフィードバックループを著しく悪化させている。

CI ログとローカルでの実測により、内訳と真因を以下の通り特定した。

| フェーズ | 時間 |
|---|---|
| コンパイル（`test` プロファイルのビルド） | 約3分 |
| テスト実行（711 tests） | **約13分（788秒）** |
| 統合テスト | ほぼ0秒 |

支配的なのは**テスト実行そのもの**であり、その大半を `secure_store` を実 Stronghold で叩く十数件のテストが消費している。他の約700件は各 0.0x 秒で終わっている。

### 真因（実測で確定）

`SecureStore` の内部で `iota_stronghold` のスナップショット read/write（`commit_with_keyprovider` / `load_snapshot`）が **1回あたり約55秒**かかる。ステップ別計測の結果は以下の通り。

| ステップ | 時間 |
|---|---|
| `Stronghold::default()` | 0.26 ms |
| `KeyProvider::try_from`（KDF相当） | 0.05 ms |
| `create_client` | 0.12 ms |
| **`commit`（スナップショット書き込み）** | **55.46 秒** |
| **`load_snapshot`（スナップショット読み込み）** | **55.49 秒** |
| `load_client` | 1.3 ms |

- 当初「Argon2 KDF が重い」と推測したが、**実測で棄却**（KDF は 0.05 ms）。
- `cargo-nextest`（プロセス並列）も試したが、Stronghold のメモリ保護処理が並列プロセス間で競合し **むしろ悪化（342秒 → 497秒）**。棄却。
- 最遅の `test_open_with_migration_migrates_legacy_snapshot` は 1 テスト内で commit/load を計6回呼ぶため、55秒 × 6 ≈ 330秒 で単体341秒に一致する。

遅延はサンドボックス/CI のようなエントロピー制約環境で顕著に出る（ローカル sandbox でも同様に遅い）。

## ゴール

1. **PR CI の高速化**: テスト実行を数秒に短縮し、CI 全体を 16分 → 3〜4分 に短縮する
2. **カバレッジ維持**: Stronghold 固有の振る舞い（鍵移行・相互復号不可）の受け入れテストは日次で担保する
3. **呼び出し側への影響最小化**: `SecureStore` を利用する50箱所超のシグネチャを変更しない

## 非ゴール

- Stronghold のスナップショット処理そのものの高速化（ライブラリ内部の問題であり、本設計の範囲外）
- CI ランナーへのエントロピー源（haveged 等）の導入（ローカル sandbox でも遅く、効果が不確実なため採用しない）

## 全体像

`SecureStore` を enum 化し、本番は Stronghold 実装、テストはインメモリ実装を切り替える。実 Stronghold を必須とする受け入れテストは `#[ignore]` で PR CI 本線から外し、日次 cron ジョブで担保する。

## アーキテクチャ

### enum 化

現行の `SecureStore` の中身（`inner: Mutex<SecureStoreInner>` と各メソッド）を `StrongholdStore` にリネーム移動し、`SecureStore` を enum として再定義する。

```rust
pub enum SecureStore {
    Stronghold(StrongholdStore),
    InMemory(Mutex<HashMap<String, Vec<u8>>>),
}
```

- `SecureStore::new(path, key)` と `SecureStore::open_with_migration(path, key)` は `Stronghold` バリアントを返す薄いラッパにする（本番コードのシグネチャは不変）。
- `SecureStore::in_memory()` を新設する（テスト専用のコンストラクタ）。
- `insert` / `get` / `delete` は `match self` で各バリアントにディスパッチする。InMemory 実装は `HashMap` の単純な読み書き。

これにより **呼び出し側（`&SecureStore` 型、`SecureStoreState(pub SecureStore)`、各コマンドの `State<SecureStoreState>`）は一切変更不要**。Tauri の `State` 管理にも手を入れない。

### テストの切り替え

- **他5ファイル**（`classifier/factory.rs`、`commands/settings_commands.rs`、`commands/account_commands.rs`、`usecase/cases/flag.rs`、`usecase/cases/mailbox.rs`）の `setup()` 内の `SecureStore::new(...stronghold, &key)` を `SecureStore::in_memory()` に置換する。不要になる `TempDir` / `key` 生成は削除する。これらは「秘密を保存して取り出せること」だけを前提にしており、実 Stronghold を必要としない。
- **`secure_store.rs` の実 Stronghold 必須テスト6件**（`test_secure_store_reopen_reads_persisted_value`、`test_open_with_migration_fresh_store`、`test_open_with_migration_reopens_with_current_key`、`test_open_with_migration_migrates_legacy_snapshot`、`test_open_with_migration_unreadable_snapshot_backed_up`、`test_stronghold_snapshots_not_cross_decryptable`）に `#[ignore = "実 Stronghold を叩くため遅い。日次 nightly-stronghold ジョブで担保"]` を付与する。これらは鍵移行・相互復号不可という Stronghold 固有の振る舞いを検証しており、InMemory では意味を持たない。

## CI 構成

### 既存 `test.yml`

`rust-test` ジョブは `cargo test` のまま。`#[ignore]` されたテストは自動的にスキップされるため、PR CI は高速化される。変更は不要（ジョブ定義そのものはそのまま）。

### 新規 `nightly-stronghold.yml`

- `schedule`（cron）で日次実行、および `workflow_dispatch`（手動実行）を許可する。
- `cargo test -- --ignored` で `#[ignore]` 付きの受け入れテストのみを実行し、Stronghold 連携の回帰を担保する。
- `test.yml` の `rust-test` と同じシステム依存関係・rust-toolchain・rust-cache のセットアップを踏襲する。

## エラーハンドリング

- InMemory 実装は `Mutex` のロック失敗のみ `AppError::Stronghold` にマップする（既存の Stronghold 実装のエラー種別に合わせる）。それ以外は成功パスのみ。
- 本番コードパス（`SecureStore::new` / `open_with_migration`）の挙動・エラーは一切変えない。

## 検証方法

1. enum 化後に `cargo test` がグリーンであること。
2. `cargo test` のテスト実行時間がローカルで**数秒**に短縮されること（実測して確認）。
3. `cargo test -- --ignored` で 6 件の受け入れテストが従来通りパスすること。
4. `cargo clippy` / `cargo fmt --check` に準拠すること。

## 期待効果

- **PR CI: 16分 → 3〜4分**（コンパイル約3分 + テスト実行数秒）。
- Stronghold 連携のカバレッジは日次 cron で維持。

## テスト観点（TDD）

- `SecureStore::in_memory()` に対する `insert` → `get` ラウンドトリップ、`delete`、未存在キーの `get` が `None` を返すこと、上書き挙動のユニットテストを追加する。
- これらは InMemory バリアントの契約が Stronghold バリアントと同等であることを保証する。
