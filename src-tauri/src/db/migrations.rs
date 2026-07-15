use crate::error::AppError;
use rusqlite::{params, Connection};

fn get_schema_version(conn: &Connection) -> Result<i32, AppError> {
    // Create schema_version table if not exists
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER NOT NULL
        );",
    )?;

    let count: i32 = conn.query_row("SELECT COUNT(*) FROM schema_version", [], |row| row.get(0))?;

    if count == 0 {
        // Check if accounts table already exists (pre-versioning DB)
        let table_exists: bool = conn.query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='accounts'",
            [],
            |row| row.get(0),
        )?;
        let initial_version = if table_exists { 1 } else { 0 };
        conn.execute(
            "INSERT INTO schema_version (version) VALUES (?1)",
            params![initial_version],
        )?;
        Ok(initial_version)
    } else {
        let version: i32 =
            conn.query_row("SELECT version FROM schema_version", [], |row| row.get(0))?;
        Ok(version)
    }
}

fn set_schema_version(conn: &Connection, version: i32) -> Result<(), AppError> {
    conn.execute("UPDATE schema_version SET version = ?1", params![version])?;
    Ok(())
}

fn migrate_v1(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS accounts (
            id          TEXT PRIMARY KEY,
            name        TEXT NOT NULL,
            email       TEXT NOT NULL,
            imap_host   TEXT NOT NULL,
            imap_port   INTEGER NOT NULL DEFAULT 993,
            smtp_host   TEXT NOT NULL,
            smtp_port   INTEGER NOT NULL DEFAULT 587,
            auth_type   TEXT NOT NULL CHECK(auth_type IN ('plain', 'oauth2')),
            created_at  DATETIME DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS mails (
            id              TEXT PRIMARY KEY,
            account_id      TEXT NOT NULL REFERENCES accounts(id),
            folder          TEXT NOT NULL,
            message_id      TEXT NOT NULL,
            in_reply_to     TEXT,
            'references'    TEXT,
            from_addr       TEXT NOT NULL,
            to_addr         TEXT NOT NULL,
            cc_addr         TEXT,
            subject         TEXT NOT NULL,
            body_text       TEXT,
            body_html       TEXT,
            date            DATETIME NOT NULL,
            has_attachments BOOLEAN DEFAULT FALSE,
            raw_size        INTEGER,
            uid             INTEGER NOT NULL,
            flags           TEXT,
            fetched_at      DATETIME DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS settings (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        ",
    )?;
    Ok(())
}

fn migrate_v2(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch(
        "ALTER TABLE accounts ADD COLUMN provider TEXT NOT NULL DEFAULT 'other'
            CHECK(provider IN ('google', 'other'));",
    )?;
    Ok(())
}

fn migrate_v3(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS projects (
            id          TEXT PRIMARY KEY,
            account_id  TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
            name        TEXT NOT NULL,
            description TEXT,
            color       TEXT,
            is_archived BOOLEAN DEFAULT FALSE,
            created_at  DATETIME DEFAULT CURRENT_TIMESTAMP,
            updated_at  DATETIME DEFAULT CURRENT_TIMESTAMP
        );

        CREATE INDEX IF NOT EXISTS idx_projects_account ON projects(account_id);

        CREATE TABLE IF NOT EXISTS mail_project_assignments (
            mail_id        TEXT PRIMARY KEY REFERENCES mails(id) ON DELETE CASCADE,
            project_id     TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            assigned_by    TEXT NOT NULL CHECK(assigned_by IN ('ai', 'user')),
            confidence     REAL,
            corrected_from TEXT,
            created_at     DATETIME DEFAULT CURRENT_TIMESTAMP
        );

        CREATE INDEX IF NOT EXISTS idx_mpa_project ON mail_project_assignments(project_id);

        CREATE TRIGGER IF NOT EXISTS trg_mpa_account_check
        BEFORE INSERT ON mail_project_assignments
        BEGIN
            SELECT CASE
                WHEN (SELECT account_id FROM mails WHERE id = NEW.mail_id)
                  != (SELECT account_id FROM projects WHERE id = NEW.project_id)
                THEN RAISE(ABORT, 'mail and project must belong to the same account')
            END;
        END;

        CREATE TRIGGER IF NOT EXISTS trg_mpa_account_check_update
        BEFORE UPDATE OF project_id ON mail_project_assignments
        BEGIN
            SELECT CASE
                WHEN (SELECT account_id FROM mails WHERE id = NEW.mail_id)
                  != (SELECT account_id FROM projects WHERE id = NEW.project_id)
                THEN RAISE(ABORT, 'mail and project must belong to the same account')
            END;
        END;

        CREATE TABLE IF NOT EXISTS correction_log (
            id             INTEGER PRIMARY KEY AUTOINCREMENT,
            mail_id        TEXT NOT NULL REFERENCES mails(id) ON DELETE CASCADE,
            from_project   TEXT REFERENCES projects(id) ON DELETE SET NULL,
            to_project     TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            corrected_at   DATETIME DEFAULT CURRENT_TIMESTAMP
        );
        ",
    )?;
    Ok(())
}

/// 1マイグレーション = (適用後のスキーマバージョン, 適用関数)
type Migration = (i32, fn(&Connection) -> Result<(), AppError>);

/// バージョン昇順に並べたマイグレーション一覧。
/// 追記時はこの配列の末尾に1行足すだけでよい。
/// v11 は別機能で予約済みのため欠番（マージ時に順序解決される）
const MIGRATIONS: &[Migration] = &[
    (1, migrate_v1),
    (2, migrate_v2),
    (3, migrate_v3),
    (4, migrate_v4),
    (5, migrate_v5),
    (6, migrate_v6),
    (7, migrate_v7),
    (8, migrate_v8),
    (9, migrate_v9),
    (10, migrate_v10),
    (12, migrate_v12),
    (13, migrate_v13),
    (14, migrate_v14),
    (15, migrate_v15),
    (16, migrate_v16),
];

pub fn run_migrations(conn: &Connection) -> Result<(), AppError> {
    apply_migrations(conn, MIGRATIONS)
}

/// マイグレーション一覧を順に適用する。
/// 各バージョンは「migrate_vN → set_schema_version(N)」を1トランザクションで
/// 包んで適用するため、途中失敗時に「スキーマ一部適用済み + version 未更新」の
/// 中途半端な状態にならない。失敗したバージョンは全体がロールバックされ、
/// 次回起動時にそのバージョンから安全に再実行できる。
fn apply_migrations(conn: &Connection, migrations: &[Migration]) -> Result<(), AppError> {
    let version = get_schema_version(conn)?;

    for &(target_version, migrate) in migrations {
        if version >= target_version {
            continue;
        }
        let tx = conn.unchecked_transaction()?;
        migrate(&tx)?;
        set_schema_version(&tx, target_version)?;
        tx.commit()?;
    }

    Ok(())
}

fn migrate_v4(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch(
        "
        CREATE VIRTUAL TABLE IF NOT EXISTS fts_mails USING fts5(
            mail_id UNINDEXED,
            subject,
            body_text,
            from_addr,
            to_addr,
            tokenize = 'trigram'
        );

        -- Auto-sync FTS on INSERT (INSERT OR REPLACE triggers DELETE then INSERT)
        CREATE TRIGGER IF NOT EXISTS trg_fts_mails_insert
        AFTER INSERT ON mails
        BEGIN
            INSERT INTO fts_mails (mail_id, subject, body_text, from_addr, to_addr)
            VALUES (NEW.id, NEW.subject, COALESCE(NEW.body_text, ''), NEW.from_addr, NEW.to_addr);
        END;

        -- Auto-sync FTS on DELETE
        CREATE TRIGGER IF NOT EXISTS trg_fts_mails_delete
        AFTER DELETE ON mails
        BEGIN
            DELETE FROM fts_mails WHERE mail_id = OLD.id;
        END;

        -- Backfill existing mails into FTS
        INSERT INTO fts_mails (mail_id, subject, body_text, from_addr, to_addr)
        SELECT id, subject, COALESCE(body_text, ''), from_addr, to_addr
        FROM mails
        WHERE id NOT IN (SELECT mail_id FROM fts_mails);
        ",
    )?;
    Ok(())
}

fn migrate_v5(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch(
        "
        -- 案件⇔ディレクトリ (1:N。UIは当面1案件1ディレクトリに制限)
        CREATE TABLE IF NOT EXISTS project_directories (
            id              TEXT PRIMARY KEY,
            project_id      TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
            path            TEXT NOT NULL UNIQUE,
            is_primary      BOOLEAN NOT NULL DEFAULT FALSE,
            status          TEXT NOT NULL DEFAULT 'ok'
                            CHECK(status IN ('ok','missing','inaccessible','error')),
            last_scanned_at DATETIME,
            created_at      DATETIME DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX IF NOT EXISTS idx_project_directories_project
            ON project_directories(project_id);
        CREATE UNIQUE INDEX IF NOT EXISTS idx_project_directories_one_primary
            ON project_directories(project_id) WHERE is_primary = TRUE;

        -- ファイルインベントリ (現在の実体のスナップショット)
        CREATE TABLE IF NOT EXISTS project_files (
            id             TEXT PRIMARY KEY,
            directory_id   TEXT NOT NULL REFERENCES project_directories(id) ON DELETE CASCADE,
            relative_path  TEXT NOT NULL,
            size_bytes     INTEGER NOT NULL,
            mtime          DATETIME NOT NULL,
            content_hash   TEXT,
            content_kind   TEXT NOT NULL DEFAULT 'none'
                           CHECK(content_kind IN ('none','text','pdf','office','other')),
            extract_status TEXT NOT NULL DEFAULT 'ok'
                           CHECK(extract_status IN ('ok','skipped_too_large','unsupported','error')),
            indexed_at     DATETIME DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(directory_id, relative_path)
        );
        CREATE INDEX IF NOT EXISTS idx_project_files_directory
            ON project_files(directory_id);

        -- クラウド送信許可ルール (デフォルト不許可、最長マッチ優先)
        CREATE TABLE IF NOT EXISTS project_cloud_rules (
            id            TEXT PRIMARY KEY,
            directory_id  TEXT NOT NULL REFERENCES project_directories(id) ON DELETE CASCADE,
            scope         TEXT NOT NULL CHECK(scope IN ('directory','file')),
            relative_path TEXT NOT NULL DEFAULT '',
            allow         BOOLEAN NOT NULL,
            created_at    DATETIME DEFAULT CURRENT_TIMESTAMP,
            UNIQUE(directory_id, scope, relative_path)
        );
        CREATE INDEX IF NOT EXISTS idx_project_cloud_rules_directory
            ON project_cloud_rules(directory_id);

        -- 案件のAIコンテキスト状態 (正本は PIGEON-CONTEXT.md、これはキャッシュ+メタ)
        CREATE TABLE IF NOT EXISTS project_contexts (
            project_id          TEXT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
            cached_context      TEXT,
            context_hash        TEXT,
            inventory_hash      TEXT,
            allow_cloud_context BOOLEAN NOT NULL DEFAULT FALSE,
            generated_at        DATETIME
        );
        ",
    )?;
    Ok(())
}

fn migrate_v6(conn: &Connection) -> Result<(), AppError> {
    // 同期の多重実行（v5以前は未ガード）で発生した重複メールを掃除してから
    // UNIQUE を張る。案件割り当てが付いている行を優先して残す
    conn.execute_batch(
        "
        DELETE FROM mails WHERE id IN (
            SELECT id FROM (
                SELECT m.id, ROW_NUMBER() OVER (
                    PARTITION BY m.account_id, m.folder, m.uid
                    ORDER BY (CASE WHEN a.mail_id IS NOT NULL THEN 0 ELSE 1 END), m.id
                ) AS rn
                FROM mails m
                LEFT JOIN mail_project_assignments a ON a.mail_id = m.id
            ) WHERE rn > 1
        );
        CREATE UNIQUE INDEX IF NOT EXISTS idx_mails_account_folder_uid
            ON mails(account_id, folder, uid);
        ",
    )?;
    Ok(())
}

fn migrate_v7(conn: &Connection) -> Result<(), AppError> {
    // 既読/未読の管理。正はサーバーの \Seen で、これはそのキャッシュ。
    // 既存行は未読(0)で初期化し、次回同期のフラグ再同期で実際の状態に収束する
    conn.execute_batch("ALTER TABLE mails ADD COLUMN is_read INTEGER NOT NULL DEFAULT 0;")?;
    Ok(())
}

fn migrate_v8(conn: &Connection) -> Result<(), AppError> {
    // 添付ファイル（オンデマンド取得・ローカルキャッシュ）
    // 詳細: docs/archive/specs/2026-07-12-attachment-download-design.md
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS attachments (
            id          TEXT PRIMARY KEY,
            mail_id     TEXT NOT NULL REFERENCES mails(id) ON DELETE CASCADE,
            filename    TEXT NOT NULL,
            mime_type   TEXT NOT NULL,
            size        INTEGER,
            file_path   TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_attachments_mail ON attachments(mail_id);
        ",
    )?;
    Ok(())
}

fn migrate_v9(conn: &Connection) -> Result<(), AppError> {
    // スター/フラグ（\Flagged）の管理。正はサーバーの \Flagged で、これはそのキャッシュ。
    // 既存行は flags 列（サーバーフラグの文字列）に \Flagged を含んでいれば 1 に埋め戻す
    // （is_read を v7 で埋め戻さなかったのは当時 flags の一括再取得が未実装だったため。
    // 今回は文字列に情報があるので活かす）
    conn.execute_batch(
        "ALTER TABLE mails ADD COLUMN is_flagged INTEGER NOT NULL DEFAULT 0;
         UPDATE mails SET is_flagged = 1 WHERE flags LIKE '%\\Flagged%';",
    )?;
    Ok(())
}

fn migrate_v10(conn: &Connection) -> Result<(), AppError> {
    // uid がサーバー実 UID として確定しているか（Sent 同期の watermark 用）。
    // 詳細: docs/archive/specs/2026-07-12-sent-sync-uidplus-design.md
    //
    // 既定は 1（確定）。INBOX 等サーバーから取得した行の uid はサーバー実 UID なので確定。
    // 一方、送信時にローカル保存する Sent 行の uid は get_max_uid+1 の推定値であり未確定。
    // 本マイグレーション以前は Sent 同期が存在しなかったため、既存の folder='Sent' 行は
    // すべて送信時の推定 uid とみなして 0（未確定）で埋め戻す。
    conn.execute_batch("ALTER TABLE mails ADD COLUMN uid_confirmed INTEGER NOT NULL DEFAULT 1;")?;
    conn.execute(
        "UPDATE mails SET uid_confirmed = 0 WHERE folder = 'Sent'",
        [],
    )?;
    Ok(())
}

fn migrate_v12(conn: &Connection) -> Result<(), AppError> {
    // ローカル下書き保存（v1: IMAP Drafts同期は将来）
    // 詳細: docs/archive/specs/2026-07-12-draft-save-design.md
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS drafts (
            id          TEXT PRIMARY KEY,
            account_id  TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
            to_addr     TEXT NOT NULL DEFAULT '',
            cc_addr     TEXT NOT NULL DEFAULT '',
            bcc_addr    TEXT NOT NULL DEFAULT '',
            subject     TEXT NOT NULL DEFAULT '',
            body_text   TEXT NOT NULL DEFAULT '',
            in_reply_to TEXT,
            created_at  DATETIME DEFAULT CURRENT_TIMESTAMP,
            updated_at  DATETIME DEFAULT CURRENT_TIMESTAMP
        );
        CREATE INDEX IF NOT EXISTS idx_drafts_account ON drafts(account_id);
        ",
    )?;
    Ok(())
}

fn migrate_v13(conn: &Connection) -> Result<(), AppError> {
    // インライン画像（cid:）の本文内表示。Content-ID を持つ添付を判別するためのカラム
    // 詳細: docs/archive/specs/2026-07-13-inline-cid-images-design.md
    conn.execute_batch(
        "
        ALTER TABLE attachments ADD COLUMN content_id TEXT;
        CREATE INDEX IF NOT EXISTS idx_attachments_content_id ON attachments(mail_id, content_id);
        ",
    )?;
    Ok(())
}

fn migrate_v14(conn: &Connection) -> Result<(), AppError> {
    // スレッド追従の除外トゥームストーン。ユーザーが分類を却下したメールを記録し、
    // auto_follow_threads がスレッド追従で黙って再割り当てするのを防ぐ。
    // メール削除時は ON DELETE CASCADE で自動的に消える。
    // 詳細: docs/archive/specs/2026-07-13-thread-follow-classify-design.md
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS follow_exclusions (
            mail_id TEXT PRIMARY KEY REFERENCES mails(id) ON DELETE CASCADE
        );
        ",
    )?;
    Ok(())
}

fn migrate_v15(conn: &Connection) -> Result<(), AppError> {
    // Reversible/Sensitive 操作の監査ログ（ADR 0004 Phase 4-4）。
    // dispatch バスが実行前に記録する。input_summary は値を切り詰めた要約で、
    // 完全な入力は保存しない（本文等の重複保存を避ける）。
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS audit_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            ts TEXT NOT NULL,
            use_case TEXT NOT NULL,
            risk TEXT NOT NULL,
            driver TEXT NOT NULL,
            input_summary TEXT NOT NULL
        );
        ",
    )?;
    Ok(())
}

fn migrate_v16(conn: &Connection) -> Result<(), AppError> {
    // Sensitive 操作の承認キュー（ADR 0004 Phase 4-4）。
    // 非 UI driver（MCP / Agent）の Sensitive 操作はここに積まれて保留される。
    // input_json は承認時の再実行に必要な完全な入力（Phase 5-2 で UI から消費）。
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS approval_queue (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            ts TEXT NOT NULL,
            use_case TEXT NOT NULL,
            input_json TEXT NOT NULL,
            driver TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending'
                CHECK (status IN ('pending', 'approved', 'rejected')),
            decided_ts TEXT
        );
        ",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// v17 相当の失敗するマイグレーション: 一部のスキーマ変更
    /// （ALTER TABLE ADD COLUMN = 非冪等）を適用した後に失敗する。
    /// 途中失敗時の原子性（ロールバック）検証用。
    fn migrate_broken_partial(conn: &Connection) -> Result<(), AppError> {
        conn.execute_batch("ALTER TABLE mails ADD COLUMN broken_col INTEGER;")?;
        // 存在しないテーブルへの INSERT で故意に失敗させる
        conn.execute("INSERT INTO no_such_table (x) VALUES (1)", [])?;
        Ok(())
    }

    /// v15 相当の正常なマイグレーション（再実行検証用）。
    /// migrate_broken_partial と同じ ALTER TABLE を含むため、
    /// 先行の失敗がロールバックされていなければ duplicate column で失敗する。
    fn migrate_fixed(conn: &Connection) -> Result<(), AppError> {
        conn.execute_batch("ALTER TABLE mails ADD COLUMN broken_col INTEGER;")?;
        Ok(())
    }

    fn column_exists(conn: &Connection, table: &str, column: &str) -> bool {
        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info(?1) WHERE name = ?2",
                params![table, column],
                |row| row.get(0),
            )
            .unwrap();
        count > 0
    }

    fn schema_version(conn: &Connection) -> i32 {
        conn.query_row("SELECT version FROM schema_version", [], |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn test_failed_migration_rolls_back_partial_changes() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        let mut with_broken: Vec<Migration> = MIGRATIONS.to_vec();
        with_broken.push((17, migrate_broken_partial));

        let result = apply_migrations(&conn, &with_broken);
        assert!(result.is_err(), "壊れたマイグレーションは失敗する");

        // 途中まで適用されたスキーマ変更がロールバックされている
        assert!(
            !column_exists(&conn, "mails", "broken_col"),
            "失敗したマイグレーションの部分適用はロールバックされる"
        );
        // schema_version は進んでいない
        assert_eq!(
            schema_version(&conn),
            16,
            "失敗したバージョンに schema_version は進まない"
        );
    }

    #[test]
    fn test_rerun_after_failure_completes_without_duplicate_column() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        let mut with_broken: Vec<Migration> = MIGRATIONS.to_vec();
        with_broken.push((17, migrate_broken_partial));
        assert!(apply_migrations(&conn, &with_broken).is_err());

        // 修正版で再実行 → duplicate column にならず完走する
        let mut with_fixed: Vec<Migration> = MIGRATIONS.to_vec();
        with_fixed.push((17, migrate_fixed));
        apply_migrations(&conn, &with_fixed)
            .expect("失敗後の再実行は duplicate column にならず完走する");

        assert!(column_exists(&conn, "mails", "broken_col"));
        assert_eq!(schema_version(&conn), 17);
    }

    #[test]
    fn test_earlier_versions_stay_committed_when_later_version_fails() {
        let conn = Connection::open_in_memory().unwrap();
        // v0 から実行し、最後のバージョンだけ失敗させる
        let mut with_broken: Vec<Migration> = MIGRATIONS.to_vec();
        with_broken.push((17, migrate_broken_partial));

        assert!(apply_migrations(&conn, &with_broken).is_err());

        // 成功済みバージョン（v1〜v16）はコミット済みのまま
        assert_eq!(
            schema_version(&conn),
            16,
            "成功したバージョンまでは確定している"
        );
        assert!(column_exists(&conn, "mails", "is_read"), "v7 は適用済み");

        // その後、通常の run_migrations は冪等に成功する
        run_migrations(&conn).unwrap();
        assert_eq!(schema_version(&conn), 16);
    }

    #[test]
    fn test_run_migrations_creates_tables() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(tables.contains(&"accounts".to_string()));
        assert!(tables.contains(&"mails".to_string()));
        assert!(tables.contains(&"settings".to_string()));
        assert!(tables.contains(&"schema_version".to_string()));
    }

    #[test]
    fn test_run_migrations_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        run_migrations(&conn).unwrap();
    }

    #[test]
    fn test_v2_migration_adds_provider_column() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        // Verify provider column exists with correct default
        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
             VALUES ('test1', 'Test', 'test@example.com', 'imap.example.com', 'smtp.example.com', 'plain')",
            [],
        ).unwrap();

        let provider: String = conn
            .query_row(
                "SELECT provider FROM accounts WHERE id = 'test1'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(provider, "other");
    }

    #[test]
    fn test_v2_migration_provider_check_constraint() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        // Valid provider 'google'
        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type, provider)
             VALUES ('g1', 'Gmail', 'user@gmail.com', 'imap.gmail.com', 'smtp.gmail.com', 'oauth2', 'google')",
            [],
        ).unwrap();

        // Invalid provider should fail
        let result = conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type, provider)
             VALUES ('x1', 'Bad', 'user@bad.com', 'imap.bad.com', 'smtp.bad.com', 'plain', 'yahoo')",
            [],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_foreign_keys_enabled() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        run_migrations(&conn).unwrap();

        // Verify foreign keys are ON: insert a mail referencing a non-existent account should fail
        let result = conn.execute(
            "INSERT INTO mails (id, account_id, folder, message_id, from_addr, to_addr, subject, date, uid)
             VALUES ('m1', 'nonexistent', 'INBOX', '<msg1>', 'a@b.com', 'c@d.com', 'Test', '2026-01-01', 1)",
            [],
        );
        assert!(
            result.is_err(),
            "foreign key constraint should have prevented insert"
        );
    }

    #[test]
    fn test_v3_migration_creates_projects_and_assignments() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        run_migrations(&conn).unwrap();

        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(tables.contains(&"projects".to_string()));
        assert!(tables.contains(&"mail_project_assignments".to_string()));
        assert!(tables.contains(&"correction_log".to_string()));

        // Verify schema version is 7 (latest)
        let version: i32 = conn
            .query_row("SELECT version FROM schema_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 16);
    }

    #[test]
    fn test_v3_migration_account_trigger_prevents_cross_account() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        run_migrations(&conn).unwrap();

        // Insert two accounts
        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
             VALUES ('acc1', 'Account 1', 'a1@example.com', 'imap.example.com', 'smtp.example.com', 'plain')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
             VALUES ('acc2', 'Account 2', 'a2@example.com', 'imap.example.com', 'smtp.example.com', 'plain')",
            [],
        ).unwrap();

        // Insert a mail belonging to acc1
        conn.execute(
            "INSERT INTO mails (id, account_id, folder, message_id, from_addr, to_addr, subject, date, uid)
             VALUES ('mail1', 'acc1', 'INBOX', '<msg1>', 'a@b.com', 'c@d.com', 'Subject', '2026-01-01', 1)",
            [],
        ).unwrap();

        // Insert a project belonging to acc2
        conn.execute(
            "INSERT INTO projects (id, account_id, name) VALUES ('proj2', 'acc2', 'Project 2')",
            [],
        )
        .unwrap();

        // Attempting to assign mail (acc1) to project (acc2) should fail
        let result = conn.execute(
            "INSERT INTO mail_project_assignments (mail_id, project_id, assigned_by)
             VALUES ('mail1', 'proj2', 'ai')",
            [],
        );
        assert!(
            result.is_err(),
            "cross-account assignment should be rejected by trigger"
        );
    }

    #[test]
    fn test_v3_migration_same_account_assignment_succeeds() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        run_migrations(&conn).unwrap();

        // Insert account
        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
             VALUES ('acc1', 'Account 1', 'a1@example.com', 'imap.example.com', 'smtp.example.com', 'plain')",
            [],
        ).unwrap();

        // Insert mail and project both belonging to acc1
        conn.execute(
            "INSERT INTO mails (id, account_id, folder, message_id, from_addr, to_addr, subject, date, uid)
             VALUES ('mail1', 'acc1', 'INBOX', '<msg1>', 'a@b.com', 'c@d.com', 'Subject', '2026-01-01', 1)",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO projects (id, account_id, name) VALUES ('proj1', 'acc1', 'Project 1')",
            [],
        )
        .unwrap();

        // Same-account assignment should succeed
        let result = conn.execute(
            "INSERT INTO mail_project_assignments (mail_id, project_id, assigned_by, confidence)
             VALUES ('mail1', 'proj1', 'ai', 0.95)",
            [],
        );
        assert!(result.is_ok(), "same-account assignment should succeed");
    }

    #[test]
    fn test_v3_cascade_delete_project() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        run_migrations(&conn).unwrap();

        // Insert account, mail, and project
        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
             VALUES ('acc1', 'Account 1', 'a1@example.com', 'imap.example.com', 'smtp.example.com', 'plain')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO mails (id, account_id, folder, message_id, from_addr, to_addr, subject, date, uid)
             VALUES ('mail1', 'acc1', 'INBOX', '<msg1>', 'a@b.com', 'c@d.com', 'Subject', '2026-01-01', 1)",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO projects (id, account_id, name) VALUES ('proj1', 'acc1', 'Project 1')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO mail_project_assignments (mail_id, project_id, assigned_by)
             VALUES ('mail1', 'proj1', 'user')",
            [],
        )
        .unwrap();

        // Verify assignment exists
        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM mail_project_assignments WHERE mail_id = 'mail1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        // Delete the project — assignment should cascade-delete
        conn.execute("DELETE FROM projects WHERE id = 'proj1'", [])
            .unwrap();

        let count_after: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM mail_project_assignments WHERE mail_id = 'mail1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            count_after, 0,
            "assignment should be cascade-deleted when project is deleted"
        );
    }

    #[test]
    fn test_v2_migration_on_existing_v1_database() {
        let conn = Connection::open_in_memory().unwrap();

        // Simulate a V1 database (tables created without provider column)
        conn.execute_batch(
            "
            CREATE TABLE accounts (
                id          TEXT PRIMARY KEY,
                name        TEXT NOT NULL,
                email       TEXT NOT NULL,
                imap_host   TEXT NOT NULL,
                imap_port   INTEGER NOT NULL DEFAULT 993,
                smtp_host   TEXT NOT NULL,
                smtp_port   INTEGER NOT NULL DEFAULT 587,
                auth_type   TEXT NOT NULL CHECK(auth_type IN ('plain', 'oauth2')),
                created_at  DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            CREATE TABLE mails (
                id              TEXT PRIMARY KEY,
                account_id      TEXT NOT NULL REFERENCES accounts(id),
                folder          TEXT NOT NULL,
                message_id      TEXT NOT NULL,
                in_reply_to     TEXT,
                'references'    TEXT,
                from_addr       TEXT NOT NULL,
                to_addr         TEXT NOT NULL,
                cc_addr         TEXT,
                subject         TEXT NOT NULL,
                body_text       TEXT,
                body_html       TEXT,
                date            DATETIME NOT NULL,
                has_attachments BOOLEAN DEFAULT FALSE,
                raw_size        INTEGER,
                uid             INTEGER NOT NULL,
                flags           TEXT,
                fetched_at      DATETIME DEFAULT CURRENT_TIMESTAMP
            );
            CREATE TABLE settings (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );
            ",
        )
        .unwrap();

        // Insert a V1 account
        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
             VALUES ('old1', 'Old Account', 'old@example.com', 'imap.example.com', 'smtp.example.com', 'plain')",
            [],
        ).unwrap();

        // Run migrations — should detect V1, apply V2 and V3
        run_migrations(&conn).unwrap();

        // Existing account should have provider = 'other'
        let provider: String = conn
            .query_row(
                "SELECT provider FROM accounts WHERE id = 'old1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(provider, "other");

        // Schema version should be 7 (latest)
        let version: i32 = conn
            .query_row("SELECT version FROM schema_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 16);
    }

    #[test]
    fn test_v4_migration_creates_fts_table() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        run_migrations(&conn).unwrap();

        // Verify fts_mails virtual table exists
        let table_exists: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='fts_mails'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            table_exists,
            "fts_mails table should exist after v4 migration"
        );

        // Schema version should be 7
        let version: i32 = conn
            .query_row("SELECT version FROM schema_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 16);
    }

    #[test]
    fn test_v4_migration_backfills_existing_mails() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();

        // Bootstrap schema_version table via get_schema_version (creates table + inserts row)
        get_schema_version(&conn).unwrap();

        // Manually run v1-v3 migrations without FTS
        migrate_v1(&conn).unwrap();
        set_schema_version(&conn, 1).unwrap();
        migrate_v2(&conn).unwrap();
        set_schema_version(&conn, 2).unwrap();
        migrate_v3(&conn).unwrap();
        set_schema_version(&conn, 3).unwrap();

        // Insert data while no FTS triggers exist
        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
             VALUES ('acc1', 'Test', 'test@example.com', 'imap.example.com', 'smtp.example.com', 'plain')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO mails (id, account_id, folder, message_id, from_addr, to_addr, subject, body_text, date, uid)
             VALUES ('m1', 'acc1', 'INBOX', '<msg1>', 'sender@example.com', 'me@example.com', 'BackfillTest subject', 'body text here', '2026-04-13', 1)",
            [],
        ).unwrap();

        // Verify no FTS table exists yet
        let fts_exists: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='fts_mails'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!fts_exists, "fts_mails should not exist before v4");

        // Now run full migrations — v4 should backfill the existing mail into FTS
        run_migrations(&conn).unwrap();

        let fts_count: i32 = conn
            .query_row("SELECT COUNT(*) FROM fts_mails", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            fts_count, 1,
            "backfill should populate fts_mails for pre-existing mails"
        );

        // Verify the backfilled content is searchable
        let search_count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM fts_mails WHERE fts_mails MATCH '\"BackfillTest\"'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(search_count, 1, "backfilled mail should be searchable");
    }

    #[test]
    fn test_v4_fts_trigger_on_insert() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
             VALUES ('acc1', 'Test', 'test@example.com', 'imap.example.com', 'smtp.example.com', 'plain')",
            [],
        ).unwrap();

        conn.execute(
            "INSERT INTO mails (id, account_id, folder, message_id, from_addr, to_addr, subject, body_text, date, uid)
             VALUES ('m1', 'acc1', 'INBOX', '<msg1>', 'alice@example.com', 'me@example.com', 'Meeting Tomorrow', 'Let us discuss the project plan', '2026-04-13', 1)",
            [],
        ).unwrap();

        // trigram tokenizer: substring match with 3+ chars
        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM fts_mails WHERE fts_mails MATCH '\"Meeting\"'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_v4_fts_trigger_on_delete() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
             VALUES ('acc1', 'Test', 'test@example.com', 'imap.example.com', 'smtp.example.com', 'plain')",
            [],
        ).unwrap();

        conn.execute(
            "INSERT INTO mails (id, account_id, folder, message_id, from_addr, to_addr, subject, body_text, date, uid)
             VALUES ('m1', 'acc1', 'INBOX', '<msg1>', 'alice@example.com', 'me@example.com', 'DeleteTarget', 'body', '2026-04-13', 1)",
            [],
        ).unwrap();

        conn.execute("DELETE FROM mails WHERE id = 'm1'", [])
            .unwrap();

        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM fts_mails WHERE fts_mails MATCH '\"DeleteTarget\"'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "FTS entry should be removed when mail is deleted");
    }

    #[test]
    fn test_v4_fts_japanese_3char_search() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
             VALUES ('acc1', 'Test', 'test@example.com', 'imap.example.com', 'smtp.example.com', 'plain')",
            [],
        ).unwrap();

        conn.execute(
            "INSERT INTO mails (id, account_id, folder, message_id, from_addr, to_addr, subject, body_text, date, uid)
             VALUES ('m1', 'acc1', 'INBOX', '<msg1>', 'sender@example.com', 'me@example.com', '見積もりの件', '予算について相談があります', '2026-04-13', 1)",
            [],
        ).unwrap();

        // trigram: 3+ char Japanese substring works via FTS
        let subject_count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM fts_mails WHERE fts_mails MATCH '\"見積もり\"'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            subject_count, 1,
            "3+ char Japanese substring search should work via FTS trigram"
        );
    }

    #[test]
    fn test_v5_migration_creates_directory_tables() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        run_migrations(&conn).unwrap();

        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(tables.contains(&"project_directories".to_string()));
        assert!(tables.contains(&"project_files".to_string()));
        assert!(tables.contains(&"project_cloud_rules".to_string()));
        assert!(tables.contains(&"project_contexts".to_string()));

        let version: i32 = conn
            .query_row("SELECT version FROM schema_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 16);
    }

    #[test]
    fn test_v5_cascade_delete_from_project() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
             VALUES ('acc1', 'A', 'a@example.com', 'imap.example.com', 'smtp.example.com', 'plain')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO projects (id, account_id, name) VALUES ('p1', 'acc1', 'Proj')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO project_directories (id, project_id, path, is_primary)
             VALUES ('d1', 'p1', '/tmp/proj1', TRUE)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO project_files (id, directory_id, relative_path, size_bytes, mtime)
             VALUES ('f1', 'd1', 'a.txt', 10, '2026-07-09T00:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO project_cloud_rules (id, directory_id, scope, relative_path, allow)
             VALUES ('r1', 'd1', 'directory', '', TRUE)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO project_contexts (project_id, cached_context) VALUES ('p1', 'ctx')",
            [],
        )
        .unwrap();

        conn.execute("DELETE FROM projects WHERE id = 'p1'", [])
            .unwrap();

        for table in [
            "project_directories",
            "project_files",
            "project_cloud_rules",
            "project_contexts",
        ] {
            let count: i32 = conn
                .query_row(&format!("SELECT COUNT(*) FROM {}", table), [], |row| {
                    row.get(0)
                })
                .unwrap();
            assert_eq!(count, 0, "{} should cascade-delete", table);
        }
    }

    #[test]
    fn test_v5_unique_path_prevents_double_link() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
             VALUES ('acc1', 'A', 'a@example.com', 'i', 's', 'plain')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO projects (id, account_id, name) VALUES ('p1', 'acc1', 'P1')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO projects (id, account_id, name) VALUES ('p2', 'acc1', 'P2')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO project_directories (id, project_id, path, is_primary)
             VALUES ('d1', 'p1', '/tmp/shared', TRUE)",
            [],
        )
        .unwrap();

        // 同じパスを別案件に紐付けると UNIQUE(path) 違反
        let result = conn.execute(
            "INSERT INTO project_directories (id, project_id, path, is_primary)
             VALUES ('d2', 'p2', '/tmp/shared', TRUE)",
            [],
        );
        assert!(
            result.is_err(),
            "same path must not be linked to two projects"
        );
    }

    #[test]
    fn test_v5_one_primary_per_project() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
             VALUES ('acc1', 'A', 'a@example.com', 'i', 's', 'plain')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO projects (id, account_id, name) VALUES ('p1', 'acc1', 'P1')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO project_directories (id, project_id, path, is_primary)
             VALUES ('d1', 'p1', '/tmp/a', TRUE)",
            [],
        )
        .unwrap();

        // 2つ目の primary は部分ユニークインデックス違反
        let result = conn.execute(
            "INSERT INTO project_directories (id, project_id, path, is_primary)
             VALUES ('d2', 'p1', '/tmp/b', TRUE)",
            [],
        );
        assert!(result.is_err(), "only one primary directory per project");
    }

    #[test]
    fn test_v7_adds_is_read_column_with_default_zero() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
             VALUES ('acc1', 'A', 'a@example.com', 'i', 's', 'plain')",
            [],
        )
        .unwrap();
        // is_read を指定しない INSERT はデフォルト 0（未読）になる
        conn.execute(
            "INSERT INTO mails (id, account_id, folder, message_id, from_addr, to_addr, subject, date, uid)
             VALUES ('m1', 'acc1', 'INBOX', '<x@y>', 'a@b', 'c@d', 'S', '2026-07-12', 1)",
            [],
        )
        .unwrap();

        let is_read: i32 = conn
            .query_row("SELECT is_read FROM mails WHERE id = 'm1'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(is_read, 0, "is_read defaults to 0 (unread)");
    }

    #[test]
    fn test_v7_existing_rows_become_unread() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        // v6 までを適用した状態で既存メールを仕込む
        get_schema_version(&conn).unwrap();
        migrate_v1(&conn).unwrap();
        migrate_v2(&conn).unwrap();
        migrate_v3(&conn).unwrap();
        migrate_v4(&conn).unwrap();
        migrate_v5(&conn).unwrap();
        migrate_v6(&conn).unwrap();
        set_schema_version(&conn, 6).unwrap();

        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
             VALUES ('acc1', 'A', 'a@example.com', 'i', 's', 'plain')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO mails (id, account_id, folder, message_id, from_addr, to_addr, subject, date, uid)
             VALUES ('m1', 'acc1', 'INBOX', '<x@y>', 'a@b', 'c@d', 'S', '2026-07-12', 1)",
            [],
        )
        .unwrap();

        run_migrations(&conn).unwrap();

        let is_read: i32 = conn
            .query_row("SELECT is_read FROM mails WHERE id = 'm1'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(is_read, 0, "v7 適用で既存行は未読になる");

        let version: i32 = conn
            .query_row("SELECT version FROM schema_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 16);
    }

    #[test]
    fn test_v10_adds_uid_confirmed_defaulting_to_one() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
             VALUES ('acc1', 'A', 'a@example.com', 'i', 's', 'plain')",
            [],
        )
        .unwrap();
        // uid_confirmed を指定しない INSERT はデフォルト 1（確定）になる
        conn.execute(
            "INSERT INTO mails (id, account_id, folder, message_id, from_addr, to_addr, subject, date, uid)
             VALUES ('m1', 'acc1', 'INBOX', '<x@y>', 'a@b', 'c@d', 'S', '2026-07-12', 1)",
            [],
        )
        .unwrap();

        let confirmed: i32 = conn
            .query_row(
                "SELECT uid_confirmed FROM mails WHERE id = 'm1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(confirmed, 1, "uid_confirmed defaults to 1 (confirmed)");
    }

    #[test]
    fn test_v10_backfills_existing_sent_rows_as_unconfirmed() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        // v8 までを適用した状態（Sent 同期が存在しなかった時代）で既存メールを仕込む
        get_schema_version(&conn).unwrap();
        migrate_v1(&conn).unwrap();
        migrate_v2(&conn).unwrap();
        migrate_v3(&conn).unwrap();
        migrate_v4(&conn).unwrap();
        migrate_v5(&conn).unwrap();
        migrate_v6(&conn).unwrap();
        migrate_v7(&conn).unwrap();
        migrate_v8(&conn).unwrap();
        set_schema_version(&conn, 8).unwrap();

        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
             VALUES ('acc1', 'A', 'a@example.com', 'i', 's', 'plain')",
            [],
        )
        .unwrap();
        // INBOX 行（サーバー実 uid）と Sent 行（送信時の推定 uid）
        conn.execute(
            "INSERT INTO mails (id, account_id, folder, message_id, from_addr, to_addr, subject, date, uid)
             VALUES ('inbox1', 'acc1', 'INBOX', '<a@y>', 'a@b', 'c@d', 'S', '2026-07-12', 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO mails (id, account_id, folder, message_id, from_addr, to_addr, subject, date, uid)
             VALUES ('sent1', 'acc1', 'Sent', '<b@y>', 'a@b', 'c@d', 'S', '2026-07-12', 1)",
            [],
        )
        .unwrap();

        run_migrations(&conn).unwrap();

        let inbox_confirmed: i32 = conn
            .query_row(
                "SELECT uid_confirmed FROM mails WHERE id = 'inbox1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let sent_confirmed: i32 = conn
            .query_row(
                "SELECT uid_confirmed FROM mails WHERE id = 'sent1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(inbox_confirmed, 1, "INBOX 既存行は確定のまま");
        assert_eq!(
            sent_confirmed, 0,
            "Sent 既存行は推定 uid として未確定に埋め戻す"
        );

        let version: i32 = conn
            .query_row("SELECT version FROM schema_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 16);
    }

    #[test]
    fn test_v6_unique_index_rejects_duplicate_uid() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
             VALUES ('acc1', 'A', 'a@example.com', 'i', 's', 'plain')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO mails (id, account_id, folder, message_id, from_addr, to_addr, subject, date, uid)
             VALUES ('m1', 'acc1', 'INBOX', '<x@y>', 'a@b', 'c@d', 'S', '2026-07-10', 100)",
            [],
        )
        .unwrap();

        // 同じ (account_id, folder, uid) の別行は UNIQUE 違反
        let result = conn.execute(
            "INSERT INTO mails (id, account_id, folder, message_id, from_addr, to_addr, subject, date, uid)
             VALUES ('m2', 'acc1', 'INBOX', '<x@y>', 'a@b', 'c@d', 'S', '2026-07-10', 100)",
            [],
        );
        assert!(
            result.is_err(),
            "same (account, folder, uid) must be rejected"
        );

        // 別フォルダなら同じ uid でも入る
        conn.execute(
            "INSERT INTO mails (id, account_id, folder, message_id, from_addr, to_addr, subject, date, uid)
             VALUES ('m3', 'acc1', 'Sent', '<x@y>', 'a@b', 'c@d', 'S', '2026-07-10', 100)",
            [],
        )
        .unwrap();
    }

    #[test]
    fn test_v6_dedupes_existing_duplicates_preferring_assigned_rows() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        run_migrations(&conn).unwrap();

        // v5 以前の状態を再現: UNIQUE を外して、同期の多重実行で入った重複を仕込む
        conn.execute_batch("DROP INDEX idx_mails_account_folder_uid;")
            .unwrap();
        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
             VALUES ('acc1', 'A', 'a@example.com', 'i', 's', 'plain')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO projects (id, account_id, name) VALUES ('p1', 'acc1', 'P')",
            [],
        )
        .unwrap();
        for id in ["m1", "m2", "m3"] {
            conn.execute(
                &format!(
                    "INSERT INTO mails (id, account_id, folder, message_id, from_addr, to_addr, subject, date, uid)
                     VALUES ('{}', 'acc1', 'INBOX', '<x@y>', 'a@b', 'c@d', 'S', '2026-07-10', 100)",
                    id
                ),
                [],
            )
            .unwrap();
        }
        // 複製のうち1行にだけ案件割り当てが付いている
        conn.execute(
            "INSERT INTO mail_project_assignments (mail_id, project_id, assigned_by)
             VALUES ('m2', 'p1', 'user')",
            [],
        )
        .unwrap();

        migrate_v6(&conn).unwrap();

        let (count, kept): (i32, String) = conn
            .query_row("SELECT COUNT(*), MIN(id) FROM mails", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap();
        assert_eq!(count, 1, "重複は1行に統合される");
        assert_eq!(kept, "m2", "割り当てが付いた行を優先して残す");

        let assignments: i32 = conn
            .query_row("SELECT COUNT(*) FROM mail_project_assignments", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(assignments, 1, "割り当ては失われない");
    }

    #[test]
    fn test_migrate_v8_creates_attachments_table() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        let table_exists: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='attachments'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(table_exists, "attachments テーブルが作成される");

        let version: i32 = conn
            .query_row("SELECT version FROM schema_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 16);
    }

    #[test]
    fn test_attachments_cascade_on_mail_delete() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        run_migrations(&conn).unwrap();
        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
             VALUES ('acc1', 'T', 't@e.com', 'imap.e.com', 'smtp.e.com', 'plain')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO mails (id, account_id, folder, message_id, from_addr, to_addr, subject, date, uid)
             VALUES ('m1', 'acc1', 'INBOX', '<x@y>', 'a@b', 'c@d', 'S', '2026-07-12', 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO attachments (id, mail_id, filename, mime_type, size, file_path)
             VALUES ('att1', 'm1', 'a.pdf', 'application/pdf', 10, '/tmp/a.pdf')",
            [],
        )
        .unwrap();

        conn.execute("DELETE FROM mails WHERE id = 'm1'", [])
            .unwrap();

        let count: i32 = conn
            .query_row("SELECT COUNT(*) FROM attachments", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0, "メール削除で添付レコードもCASCADE削除される");
    }

    #[test]
    fn test_v9_adds_is_flagged_column_with_default_zero() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
             VALUES ('acc1', 'A', 'a@example.com', 'i', 's', 'plain')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO mails (id, account_id, folder, message_id, from_addr, to_addr, subject, date, uid)
             VALUES ('m1', 'acc1', 'INBOX', '<x@y>', 'a@b', 'c@d', 'S', '2026-07-13', 1)",
            [],
        )
        .unwrap();

        let is_flagged: i32 = conn
            .query_row("SELECT is_flagged FROM mails WHERE id = 'm1'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(is_flagged, 0, "is_flagged defaults to 0 (未フラグ)");
    }

    #[test]
    fn test_v9_backfills_is_flagged_from_existing_flags_column() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        // v8 までを適用した状態で既存メールを仕込む（flags 列にサーバーフラグ文字列がある想定）
        get_schema_version(&conn).unwrap();
        migrate_v1(&conn).unwrap();
        migrate_v2(&conn).unwrap();
        migrate_v3(&conn).unwrap();
        migrate_v4(&conn).unwrap();
        migrate_v5(&conn).unwrap();
        migrate_v6(&conn).unwrap();
        migrate_v7(&conn).unwrap();
        migrate_v8(&conn).unwrap();
        set_schema_version(&conn, 8).unwrap();

        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
             VALUES ('acc1', 'A', 'a@example.com', 'i', 's', 'plain')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO mails (id, account_id, folder, message_id, from_addr, to_addr, subject, date, uid, flags)
             VALUES ('m1', 'acc1', 'INBOX', '<x@y>', 'a@b', 'c@d', 'S', '2026-07-13', 1, '\\Seen \\Flagged')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO mails (id, account_id, folder, message_id, from_addr, to_addr, subject, date, uid, flags)
             VALUES ('m2', 'acc1', 'INBOX', '<p@q>', 'a@b', 'c@d', 'S', '2026-07-13', 2, '\\Seen')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO mails (id, account_id, folder, message_id, from_addr, to_addr, subject, date, uid, flags)
             VALUES ('m3', 'acc1', 'INBOX', '<r@s>', 'a@b', 'c@d', 'S', '2026-07-13', 3, NULL)",
            [],
        )
        .unwrap();

        run_migrations(&conn).unwrap();

        let flagged = |id: &str| -> i32 {
            conn.query_row(
                &format!("SELECT is_flagged FROM mails WHERE id = '{}'", id),
                [],
                |row| row.get(0),
            )
            .unwrap()
        };
        assert_eq!(
            flagged("m1"),
            1,
            "flags に \\Flagged を含む行は埋め戻される"
        );
        assert_eq!(flagged("m2"), 0, "\\Flagged を含まない行は 0 のまま");
        assert_eq!(flagged("m3"), 0, "flags が NULL の行は 0 のまま");

        let version: i32 = conn
            .query_row("SELECT version FROM schema_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 16);
    }

    #[test]
    fn test_migrate_v12_creates_drafts_table() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        let table_exists: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='drafts'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(table_exists, "drafts テーブルが作成される");

        let version: i32 = conn
            .query_row("SELECT version FROM schema_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 16);
    }

    #[test]
    fn test_drafts_defaults_and_cascade_on_account_delete() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        run_migrations(&conn).unwrap();

        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
             VALUES ('acc1', 'A', 'a@example.com', 'i', 's', 'plain')",
            [],
        )
        .unwrap();

        // 必須項目（id, account_id）だけ指定すれば残りはデフォルトで空文字になる
        conn.execute(
            "INSERT INTO drafts (id, account_id) VALUES ('d1', 'acc1')",
            [],
        )
        .unwrap();

        let (to_addr, subject): (String, String) = conn
            .query_row(
                "SELECT to_addr, subject FROM drafts WHERE id = 'd1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(to_addr, "");
        assert_eq!(subject, "");

        conn.execute("DELETE FROM accounts WHERE id = 'acc1'", [])
            .unwrap();

        let count: i32 = conn
            .query_row("SELECT COUNT(*) FROM drafts", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0, "アカウント削除で下書きもCASCADE削除される");
    }

    #[test]
    fn test_v14_follow_exclusions_table_and_cascade() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        run_migrations(&conn).unwrap();

        let table_exists: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master
                 WHERE type='table' AND name='follow_exclusions'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(table_exists, "follow_exclusions テーブルが作成される");

        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
             VALUES ('acc1', 'A', 'a@example.com', 'i', 's', 'plain')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO mails (id, account_id, folder, message_id, from_addr, to_addr, subject, date, uid)
             VALUES ('m1', 'acc1', 'INBOX', '<m1@ex.com>', 'x@ex.com', 'y@ex.com', 'S', '2026-07-13T00:00:00', 1)",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO follow_exclusions (mail_id) VALUES ('m1')", [])
            .unwrap();

        // メール削除で除外行も CASCADE 削除される
        conn.execute("DELETE FROM mails WHERE id = 'm1'", [])
            .unwrap();
        let count: i32 = conn
            .query_row("SELECT COUNT(*) FROM follow_exclusions", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(
            count, 0,
            "メール削除で除外トゥームストーンもCASCADE削除される"
        );
    }
}
