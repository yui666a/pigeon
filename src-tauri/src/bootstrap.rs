//! GUI（Tauri）と CLI / MCP プロセスで共通の初期化処理。
//!
//! データディレクトリの解決、DB の open とマイグレーション、SecureStore の
//! マスター鍵解決と open を一箇所にまとめる。**手順が食い違うとデータが
//! 読めなくなる**ため、両者から必ずこの関数を呼ぶこと。

use crate::db::migrations;
use crate::error::AppError;
use crate::secure_store::{self, MasterKeyMigration, SecureStore};
use rusqlite::Connection;
use std::path::{Path, PathBuf};

/// アプリのデータディレクトリ（`~/Library/Application Support/Pigeon` 等）を作って返す。
pub fn resolve_data_dir() -> Result<PathBuf, AppError> {
    let data_dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Pigeon");
    std::fs::create_dir_all(&data_dir).map_err(|e| {
        AppError::FileIo(format!(
            "failed to create data directory {}: {e}",
            data_dir.display()
        ))
    })?;
    Ok(data_dir)
}

/// `pigeon.db` を開き、拡張登録・外部キー有効化・マイグレーションまで済ませる。
pub fn open_db(data_dir: &Path) -> Result<Connection, AppError> {
    let db_path = data_dir.join("pigeon.db");
    crate::db::vec_ext::register();
    let conn = Connection::open(&db_path)?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    migrations::run_migrations(&conn)?;
    Ok(conn)
}

/// SecureStore を開く。マスター鍵はデバイス固有の乱数を OS キーチェーンに
/// 保管する（ADR 0003）。旧固定鍵のスナップショットは開いた時点で新鍵へ
/// 再暗号化する。
pub fn open_secure_store(data_dir: &Path) -> Result<(SecureStore, MasterKeyMigration), AppError> {
    let key_backend = secure_store::default_master_key_backend(data_dir);
    let key = secure_store::resolve_master_key(key_backend.as_ref())?;
    let stronghold_path = data_dir.join("pigeon.stronghold");
    SecureStore::open_with_migration(stronghold_path, &key)
}

/// マスター鍵移行の結果を stderr に通知する。GUI / CLI で表示を揃える。
pub fn report_master_key_migration(migration: &MasterKeyMigration) {
    match migration {
        MasterKeyMigration::MigratedFromLegacy => {
            eprintln!("[info] secure store: 旧固定鍵のスナップショットを新しいマスター鍵で再暗号化しました");
        }
        MasterKeyMigration::UnreadableBackedUp { backup } => {
            eprintln!(
                "[warn] secure store: 既存スナップショットを復号できなかったため {} に退避しました。アカウントの再認証が必要です",
                backup.display()
            );
        }
        _ => {}
    }
}
