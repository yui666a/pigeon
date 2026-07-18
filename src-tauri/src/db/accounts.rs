use crate::error::AppError;
use crate::models::account::{Account, AccountProvider, AuthType, CreateAccountRequest};
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

pub fn insert_account(conn: &Connection, req: &CreateAccountRequest) -> Result<Account, AppError> {
    let id = Uuid::new_v4().to_string();
    insert_account_with_id(conn, &id, req)
}

pub fn insert_account_with_id(
    conn: &Connection,
    id: &str,
    req: &CreateAccountRequest,
) -> Result<Account, AppError> {
    conn.execute(
        "INSERT INTO accounts (id, name, email, imap_host, imap_port, smtp_host, smtp_port, auth_type, provider)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            id, req.name, req.email, req.imap_host, req.imap_port,
            req.smtp_host, req.smtp_port, req.auth_type.as_str(), req.provider.as_str()
        ],
    )?;
    get_account(conn, id)
}

/// accounts テーブルの1行を Account へ変換する共通マッパー。
/// カラム順は `SELECT id, name, email, imap_host, imap_port, smtp_host, smtp_port,
/// auth_type, provider, created_at` に一致させること。
///
/// 未知の auth_type / provider は Plain / Other にフォールバックするが、
/// 黙って丸めると OAuth アカウントが Plain 扱いになり原因不明の認証失敗に
/// つながるため（B-10）、必ず警告ログを残す。エラー化しないのは、新しい値を
/// 書く将来バージョンの DB を旧バージョンで開いた場合にアカウント一覧全体が
/// 読めなくなるのを避けるため。
fn row_to_account(row: &rusqlite::Row<'_>) -> rusqlite::Result<Account> {
    let id: String = row.get(0)?;
    let auth_str: String = row.get(7)?;
    let provider_str: String = row.get(8)?;
    let auth_type = AuthType::try_from(auth_str.as_str()).unwrap_or_else(|e| {
        eprintln!(
            "[warn] account {}: {} — falling back to auth_type=plain",
            id, e
        );
        AuthType::Plain
    });
    let provider = AccountProvider::try_from(provider_str.as_str()).unwrap_or_else(|e| {
        eprintln!(
            "[warn] account {}: {} — falling back to provider=other",
            id, e
        );
        AccountProvider::Other
    });
    Ok(Account {
        id,
        name: row.get(1)?,
        email: row.get(2)?,
        imap_host: row.get(3)?,
        imap_port: row.get::<_, u32>(4)? as u16,
        smtp_host: row.get(5)?,
        smtp_port: row.get::<_, u32>(6)? as u16,
        auth_type,
        provider,
        created_at: row.get(9)?,
        needs_reauth: false,
    })
}

pub fn get_account(conn: &Connection, id: &str) -> Result<Account, AppError> {
    conn.query_row(
        "SELECT id, name, email, imap_host, imap_port, smtp_host, smtp_port, auth_type, provider, created_at
         FROM accounts WHERE id = ?1",
        params![id],
        row_to_account,
    ).map_err(|_| AppError::AccountNotFound(id.to_string()))
}

pub fn list_accounts(conn: &Connection) -> Result<Vec<Account>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, name, email, imap_host, imap_port, smtp_host, smtp_port, auth_type, provider, created_at
         FROM accounts ORDER BY created_at",
    )?;
    let accounts = stmt
        .query_map([], row_to_account)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(accounts)
}

pub fn delete_account(conn: &Connection, id: &str) -> Result<(), AppError> {
    // 複数テーブルの削除を原子的に行う（途中失敗でメールだけ消えた
    // 中途半端な状態を残さない）。
    // V1のmailsテーブルにはON DELETE CASCADEがないため、先に手動で削除する。
    // それ以外の関連は FK の CASCADE で消える:
    //   mails → mail_project_assignments / correction_log / attachments / follow_exclusions
    //   accounts → projects / drafts、projects → project_directories → project_files 等
    //   fts_mails は db::fts::remove_account_mails で先に削除する（v17 でトリガー廃止）
    let tx = conn.unchecked_transaction()?;
    crate::db::fts::remove_account_mails(&tx, id)?;
    crate::db::chunks::remove_account_vectors(&tx, id)?;
    tx.execute("DELETE FROM mails WHERE account_id = ?1", params![id])?;
    tx.execute("DELETE FROM projects WHERE account_id = ?1", params![id])?;
    let affected = tx.execute("DELETE FROM accounts WHERE id = ?1", params![id])?;
    if affected == 0 {
        return Err(AppError::AccountNotFound(id.to_string()));
    }
    tx.commit()?;
    Ok(())
}

pub fn account_exists_by_email(
    conn: &Connection,
    email: &str,
) -> Result<Option<Account>, AppError> {
    let account = conn
        .query_row(
            "SELECT id, name, email, imap_host, imap_port, smtp_host, smtp_port, auth_type, provider, created_at
             FROM accounts WHERE email = ?1",
            params![email],
            row_to_account,
        )
        .optional()?;
    Ok(account)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrations::run_migrations;

    fn setup_db() -> Connection {
        crate::db::vec_ext::register();
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        conn
    }

    fn sample_request() -> CreateAccountRequest {
        CreateAccountRequest {
            name: "Test Account".into(),
            email: "test@example.com".into(),
            imap_host: "imap.example.com".into(),
            imap_port: 993,
            smtp_host: "smtp.example.com".into(),
            smtp_port: 587,
            auth_type: AuthType::Plain,
            provider: AccountProvider::Other,
            password: Some("secret".into()),
        }
    }

    fn sample_google_request() -> CreateAccountRequest {
        CreateAccountRequest {
            name: "Gmail Account".into(),
            email: "user@gmail.com".into(),
            imap_host: "imap.gmail.com".into(),
            imap_port: 993,
            smtp_host: "smtp.gmail.com".into(),
            smtp_port: 587,
            auth_type: AuthType::Oauth2,
            provider: AccountProvider::Google,
            password: None,
        }
    }

    #[test]
    fn test_insert_and_get_account() {
        let conn = setup_db();
        let account = insert_account(&conn, &sample_request()).unwrap();
        assert_eq!(account.name, "Test Account");
        assert_eq!(account.email, "test@example.com");
        assert_eq!(account.provider, AccountProvider::Other);
        let fetched = get_account(&conn, &account.id).unwrap();
        assert_eq!(fetched.id, account.id);
        assert_eq!(fetched.provider, AccountProvider::Other);
    }

    #[test]
    fn test_insert_google_account() {
        let conn = setup_db();
        let account = insert_account(&conn, &sample_google_request()).unwrap();
        assert_eq!(account.provider, AccountProvider::Google);
        assert!(matches!(account.auth_type, AuthType::Oauth2));
        assert_eq!(account.imap_host, "imap.gmail.com");
    }

    #[test]
    fn test_insert_account_with_id() {
        let conn = setup_db();
        let id = "custom-id-123";
        let account = insert_account_with_id(&conn, id, &sample_google_request()).unwrap();
        assert_eq!(account.id, id);
        assert_eq!(account.provider, AccountProvider::Google);
    }

    #[test]
    fn test_list_accounts() {
        let conn = setup_db();
        insert_account(&conn, &sample_request()).unwrap();
        insert_account(&conn, &sample_google_request()).unwrap();
        let accounts = list_accounts(&conn).unwrap();
        assert_eq!(accounts.len(), 2);
    }

    #[test]
    fn test_delete_account() {
        let conn = setup_db();
        let account = insert_account(&conn, &sample_request()).unwrap();
        delete_account(&conn, &account.id).unwrap();
        let result = get_account(&conn, &account.id);
        assert!(result.is_err());
    }

    /// acc1 のメール・案件・割り当て・下書きを一式作る（delete_account テスト用）。
    /// 共有 setup_db（FK 有効・acc1 作成済み）の接続を前提とする。
    fn seed_account_data(conn: &Connection) {
        let mail = crate::test_helpers::make_mail(
            "m1",
            "<m1@example.com>",
            "Subject",
            "2026-04-13T10:00:00",
        );
        crate::db::mails::insert_mail(conn, &mail).unwrap();
        crate::db::projects::insert_project_with_id(conn, "proj1", "acc1", "Proj", None, None)
            .unwrap();
        crate::db::assignments::assign_mail(conn, "m1", "proj1", "user", None).unwrap();
        conn.execute(
            "INSERT INTO drafts (id, account_id, subject) VALUES ('d1', 'acc1', 'Draft')",
            [],
        )
        .unwrap();
    }

    fn count_rows(conn: &Connection, table: &str) -> i64 {
        conn.query_row(&format!("SELECT COUNT(*) FROM {}", table), [], |row| {
            row.get(0)
        })
        .unwrap()
    }

    #[test]
    fn test_delete_account_removes_all_related_rows() {
        // B-4: mails は手動 DELETE、projects/drafts は accounts からの CASCADE、
        // assignments は mails からの CASCADE で、関連データが漏れなく消えること
        let conn = crate::test_helpers::setup_db();
        seed_account_data(&conn);

        delete_account(&conn, "acc1").unwrap();

        assert_eq!(count_rows(&conn, "accounts"), 0);
        assert_eq!(count_rows(&conn, "mails"), 0);
        assert_eq!(count_rows(&conn, "projects"), 0);
        assert_eq!(count_rows(&conn, "mail_project_assignments"), 0);
        assert_eq!(count_rows(&conn, "drafts"), 0);
    }

    #[test]
    fn test_delete_account_rolls_back_atomically_on_mid_failure() {
        // B-4: mails/projects の削除成功後に accounts の削除が失敗したら、
        // 全体がロールバックされて中途半端な削除が残らないこと
        let conn = crate::test_helpers::setup_db();
        seed_account_data(&conn);

        // 失敗注入: 最後の書き込み（accounts の DELETE）だけを失敗させる
        conn.execute_batch(
            "CREATE TRIGGER fail_account_delete BEFORE DELETE ON accounts
             BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
        )
        .unwrap();

        let result = delete_account(&conn, "acc1");
        assert!(result.is_err(), "注入した失敗がエラーとして伝播する");

        assert_eq!(count_rows(&conn, "accounts"), 1, "アカウントは残る");
        assert_eq!(
            count_rows(&conn, "mails"),
            1,
            "メールの削除がロールバックされる"
        );
        assert_eq!(
            count_rows(&conn, "projects"),
            1,
            "案件の削除がロールバックされる"
        );
        assert_eq!(
            count_rows(&conn, "mail_project_assignments"),
            1,
            "案件割り当ても失われない"
        );
    }

    #[test]
    fn test_get_nonexistent_account() {
        let conn = setup_db();
        let result = get_account(&conn, "nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_account_exists_by_email_found() {
        let conn = setup_db();
        insert_account(&conn, &sample_request()).unwrap();
        let result = account_exists_by_email(&conn, "test@example.com").unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().email, "test@example.com");
    }

    #[test]
    fn test_account_exists_by_email_not_found() {
        let conn = setup_db();
        let result = account_exists_by_email(&conn, "nonexistent@example.com").unwrap();
        assert!(result.is_none());
    }
}
