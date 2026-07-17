//! 書き込み系 DB 操作のトランザクションヘルパ。
//! mails 行と fts_mails 索引のように複数文で1つの状態遷移を表す操作を
//! 原子化するために使う（v17 で FTS の SQL トリガー同期を廃止したため、
//! 2文が別々にコミットされると索引の欠損・孤児が生じ得る）。

use crate::error::AppError;
use rusqlite::Connection;

/// `f` をトランザクション内で実行する。
/// 呼び出し元が既にトランザクション中（autocommit でない）の場合、SQLite は
/// ネストした BEGIN を許さないため、新たに開かず呼び出し元のトランザクションに
/// 相乗りする（原子性は外側のトランザクションが担保する）。
pub(crate) fn with_tx<T>(
    conn: &Connection,
    f: impl FnOnce(&Connection) -> Result<T, AppError>,
) -> Result<T, AppError> {
    if conn.is_autocommit() {
        let tx = conn.unchecked_transaction()?;
        let result = f(&tx)?;
        tx.commit()?;
        Ok(result)
    } else {
        f(conn)
    }
}
