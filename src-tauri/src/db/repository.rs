//! メール永続化の port（`MailRepository` trait）と SQLite 実装。
//!
//! `db/*.rs` は `&Connection` 第一引数の関数型リポジトリだが、そのままでは
//! ユースケース層（mail_sync/sync_service や commands）のテストに実 DB が必須になる。
//! LLM 境界が `classifier::LlmClassifier`（trait）で抽象化されているのと同様に、
//! DB 境界もこの trait で抽象化し、ユースケース関数が `&dyn MailRepository` を
//! 受けることでモック差し替え可能にする。
//!
//! メソッドは全 CRUD の一括 trait 化はせず、「ユースケース層から呼ばれ、
//! モックでの検証価値が高いもの」から最小セットで始める（現状は
//! mail_sync/sync_service が消費する同期系のみ）。SQLite 実装
//! `SqliteMailRepository` は既存の自由関数（`db::mails` / `db::sent_sync`）へ
//! 委譲するだけで、SQL は一切持たない。
//!
//! ライフタイム設計: 接続は `DbState(Mutex<Connection>)` が所有し、ロックは
//! `with_conn` クロージャのスコープに閉じる。そのためリポジトリは接続を
//! 所有せず `&Connection` を借用し、クロージャ内で都度構築する
//! （`SqliteMailRepository::new(conn)`）。await を跨いで保持しない。

use std::collections::HashMap;

use rusqlite::Connection;

use crate::db::{mails, sent_sync};
use crate::error::AppError;
use crate::models::mail::Mail;

/// メール永続化の port。ユースケース関数は `&dyn MailRepository` で受け、
/// 本番は `SqliteMailRepository`、テストはモックを注入する。
pub trait MailRepository {
    /// メールを挿入する。同じ (account_id, folder, uid) の行が既にあれば
    /// 何もせず false を返す（挿入したら true）。
    fn insert_mail(&self, mail: &Mail) -> Result<bool, AppError>;

    /// Sent 同期用: message_id で既存行があれば uid を確定更新、
    /// 無ければ挿入する。新規挿入時のみ true。
    fn upsert_sent_mail(&self, mail: &Mail) -> Result<bool, AppError>;

    /// folder 内の最大 uid（行が無ければ 0）。差分同期の watermark 用。
    fn get_max_uid(&self, account_id: &str, folder: &str) -> Result<u32, AppError>;

    /// uid_confirmed=1 の行のみでの folder 内最大 uid（行が無ければ 0）。
    /// Sent の watermark 用（推定 uid による汚染を防ぐ）。
    fn get_max_confirmed_uid(&self, account_id: &str, folder: &str) -> Result<u32, AppError>;

    /// folder 内の最小 uid（行が無ければ 0）。バックフィルの起点用。
    fn get_min_uid(&self, account_id: &str, folder: &str) -> Result<u32, AppError>;

    /// uid → (\Seen, \Flagged) マップで既知メールの is_read / is_flagged を
    /// 一括更新し、更新した行数を返す（フラグ再同期用）。
    fn update_flag_state(
        &self,
        account_id: &str,
        folder: &str,
        flags_by_uid: &HashMap<u32, (bool, bool)>,
    ) -> Result<usize, AppError>;
}

/// SQLite 実装。既存の自由関数（`db::mails` / `db::sent_sync`）へ委譲する。
pub struct SqliteMailRepository<'a> {
    conn: &'a Connection,
}

impl<'a> SqliteMailRepository<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }
}

impl MailRepository for SqliteMailRepository<'_> {
    fn insert_mail(&self, mail: &Mail) -> Result<bool, AppError> {
        mails::insert_mail(self.conn, mail)
    }

    fn upsert_sent_mail(&self, mail: &Mail) -> Result<bool, AppError> {
        sent_sync::upsert_sent_mail(self.conn, mail)
    }

    fn get_max_uid(&self, account_id: &str, folder: &str) -> Result<u32, AppError> {
        mails::get_max_uid(self.conn, account_id, folder)
    }

    fn get_max_confirmed_uid(&self, account_id: &str, folder: &str) -> Result<u32, AppError> {
        mails::get_max_confirmed_uid(self.conn, account_id, folder)
    }

    fn get_min_uid(&self, account_id: &str, folder: &str) -> Result<u32, AppError> {
        mails::get_min_uid(self.conn, account_id, folder)
    }

    fn update_flag_state(
        &self,
        account_id: &str,
        folder: &str,
        flags_by_uid: &HashMap<u32, (bool, bool)>,
    ) -> Result<usize, AppError> {
        mails::update_flag_state(self.conn, account_id, folder, flags_by_uid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{make_mail, setup_db};

    /// SqliteMailRepository が既存関数へ正しく委譲していることを、
    /// 実 SQLite（in-memory）で end-to-end に確認する。
    #[test]
    fn test_sqlite_repository_delegates_insert_and_max_uid() {
        let conn = setup_db();
        let repo = SqliteMailRepository::new(&conn);

        let mut mail = make_mail("m1", "<m1@ex.com>", "Hello", "2026-07-13T10:00:00");
        mail.uid = 7;
        assert!(repo.insert_mail(&mail).unwrap(), "初回は挿入される");
        assert!(!repo.insert_mail(&mail).unwrap(), "重複は無視される");

        assert_eq!(repo.get_max_uid("acc1", "INBOX").unwrap(), 7);
        assert_eq!(repo.get_min_uid("acc1", "INBOX").unwrap(), 7);
        assert_eq!(
            repo.get_max_uid("acc1", "Sent").unwrap(),
            0,
            "別フォルダは対象外"
        );
    }

    #[test]
    fn test_sqlite_repository_confirmed_uid_excludes_estimated_rows() {
        let conn = setup_db();
        let repo = SqliteMailRepository::new(&conn);

        let mut estimated = make_mail("m1", "<m1@ex.com>", "A", "2026-07-13T10:00:00");
        estimated.folder = "Sent".into();
        estimated.uid = 100;
        estimated.uid_confirmed = false;
        let mut confirmed = make_mail("m2", "<m2@ex.com>", "B", "2026-07-13T11:00:00");
        confirmed.folder = "Sent".into();
        confirmed.uid = 40;
        confirmed.uid_confirmed = true;
        repo.insert_mail(&estimated).unwrap();
        repo.insert_mail(&confirmed).unwrap();

        assert_eq!(repo.get_max_confirmed_uid("acc1", "Sent").unwrap(), 40);
    }

    #[test]
    fn test_sqlite_repository_delegates_upsert_sent_mail() {
        let conn = setup_db();
        let repo = SqliteMailRepository::new(&conn);

        let mut sent = make_mail("m1", "<m1@ex.com>", "Sent mail", "2026-07-13T10:00:00");
        sent.folder = "Sent".into();
        sent.uid = 3;
        assert!(repo.upsert_sent_mail(&sent).unwrap(), "新規は挿入される");

        // 同じ message_id はサーバー行として uid 確定更新のみ（新規扱いしない）
        let mut server_copy = make_mail("m2", "<m1@ex.com>", "Sent mail", "2026-07-13T10:00:00");
        server_copy.folder = "Sent".into();
        server_copy.uid = 9;
        assert!(!repo.upsert_sent_mail(&server_copy).unwrap());
    }

    #[test]
    fn test_sqlite_repository_delegates_update_flag_state() {
        let conn = setup_db();
        let repo = SqliteMailRepository::new(&conn);

        let mut mail = make_mail("m1", "<m1@ex.com>", "Hello", "2026-07-13T10:00:00");
        mail.uid = 5;
        mail.is_read = false;
        repo.insert_mail(&mail).unwrap();

        let mut flags = HashMap::new();
        flags.insert(5u32, (true, true));
        assert_eq!(repo.update_flag_state("acc1", "INBOX", &flags).unwrap(), 1);
        assert_eq!(
            repo.update_flag_state("acc1", "INBOX", &flags).unwrap(),
            0,
            "状態が変わらない行は更新しない"
        );
    }
}
