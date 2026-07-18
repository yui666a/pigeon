//! sqlite-vec 拡張の登録。auto_extension はプロセス全体設定のため Once で1回だけ行う。
//! run_migrations の冒頭から呼ばれるので、本番（lib.rs）・テスト（setup_db）の
//! どの接続でも vec0 仮想テーブルが使える。
//! 注意: rusqlite を 0.34+ に上げる際は register_auto_extension API への
//! 書き換えが必要（sqlite-vec issue #206）。

use rusqlite::ffi::sqlite3_auto_extension;
use sqlite_vec::sqlite3_vec_init;
use std::sync::Once;

static REGISTER: Once = Once::new();

pub fn register() {
    REGISTER.call_once(|| unsafe {
        sqlite3_auto_extension(Some(std::mem::transmute(sqlite3_vec_init as *const ())));
    });
}
