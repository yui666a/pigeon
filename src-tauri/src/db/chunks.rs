//! mail_chunks（チャンク本体・埋め込みキュー）と vec_chunks（sqlite-vec 索引）の CRUD。
//! 「embedded_at IS NULL = 未埋め込みキュー」として embedding::worker が消化する。
//! mails 行の削除時は remove_mail_vectors / remove_account_vectors で
//! vec_chunks を先に消すこと（mail_chunks は FK CASCADE で消えるが、
//! vec0 仮想テーブルは FK に参加しないため明示削除が必要）。

use crate::error::AppError;
use rusqlite::{params, Connection};
use zerocopy::IntoBytes;

pub struct PendingChunk {
    pub id: i64,
    pub content: String,
}

pub fn insert_chunks(conn: &Connection, mail_id: &str, chunks: &[String]) -> Result<(), AppError> {
    crate::db::tx::with_tx(conn, |conn| {
        let mut stmt = conn.prepare(
            "INSERT INTO mail_chunks (mail_id, chunk_index, content) VALUES (?1, ?2, ?3)",
        )?;
        for (i, content) in chunks.iter().enumerate() {
            stmt.execute(params![mail_id, i as i64, content])?;
        }
        Ok(())
    })
}

pub fn mails_without_chunks(
    conn: &Connection,
    limit: u32,
) -> Result<Vec<(String, String, Option<String>)>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT m.id, m.subject, m.body_text FROM mails m
         WHERE NOT EXISTS (SELECT 1 FROM mail_chunks c WHERE c.mail_id = m.id)
         ORDER BY m.date DESC
         LIMIT ?1",
    )?;
    let rows = stmt
        .query_map(params![limit], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn pending_chunks(conn: &Connection, limit: u32) -> Result<Vec<PendingChunk>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, content FROM mail_chunks
         WHERE embedded_at IS NULL
         ORDER BY id
         LIMIT ?1",
    )?;
    let rows = stmt
        .query_map(params![limit], |r| {
            Ok(PendingChunk {
                id: r.get(0)?,
                content: r.get(1)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// 埋め込みベクトルを保存し、対応する mail_chunks 行に embedded_at を立てる。
/// UPDATE を先に行い影響行数を見る: embed の HTTP 呼び出し中にメール（と
/// mail_chunks 行、CASCADE 経由）が削除されていた場合は 0 行にマッチするので、
/// vec_chunks への INSERT をスキップして Ok を返す（vec0 は FK に参加しないため、
/// 先に INSERT すると mail_chunks 側が消えていてもベクトルだけが孤児として残る）。
pub fn store_embedding(
    conn: &Connection,
    chunk_id: i64,
    embedding: &[f32],
) -> Result<(), AppError> {
    crate::db::tx::with_tx(conn, |conn| {
        let affected = conn.execute(
            "UPDATE mail_chunks SET embedded_at = datetime('now') WHERE id = ?1",
            params![chunk_id],
        )?;
        if affected == 0 {
            // チャンクは埋め込み中に削除済み。索引すべき対象がないので何もしない。
            return Ok(());
        }
        conn.execute(
            "INSERT INTO vec_chunks (chunk_id, embedding) VALUES (?1, ?2)",
            params![chunk_id, embedding.as_bytes()],
        )?;
        Ok(())
    })
}

pub fn pending_totals(conn: &Connection) -> Result<(u64, u64), AppError> {
    let unchunked: u64 = conn.query_row(
        "SELECT COUNT(*) FROM mails m
         WHERE NOT EXISTS (SELECT 1 FROM mail_chunks c WHERE c.mail_id = m.id)",
        [],
        |r| r.get(0),
    )?;
    let unembedded: u64 = conn.query_row(
        "SELECT COUNT(*) FROM mail_chunks WHERE embedded_at IS NULL",
        [],
        |r| r.get(0),
    )?;
    Ok((unchunked, unembedded))
}

pub fn remove_mail_vectors(conn: &Connection, mail_id: &str) -> Result<(), AppError> {
    conn.execute(
        "DELETE FROM vec_chunks WHERE chunk_id IN
         (SELECT id FROM mail_chunks WHERE mail_id = ?1)",
        params![mail_id],
    )?;
    Ok(())
}

pub fn remove_account_vectors(conn: &Connection, account_id: &str) -> Result<(), AppError> {
    conn.execute(
        "DELETE FROM vec_chunks WHERE chunk_id IN
         (SELECT c.id FROM mail_chunks c
          JOIN mails m ON c.mail_id = m.id
          WHERE m.account_id = ?1)",
        params![account_id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{accounts, mails};
    use crate::test_helpers::{make_mail, setup_db};

    fn vec_row_count(conn: &Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM vec_chunks", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn test_v18_creates_tables_and_vec0_works() {
        let conn = setup_db();
        // vec0 仮想テーブルが機能する（拡張ロード＋DDLの検証）
        // cosine 距離はゼロベクトルで未定義のため、非ゼロのベクトルを入れる
        conn.execute(
            "INSERT INTO vec_chunks (chunk_id, embedding) VALUES (1, ?1)",
            params![vec![1.0f32; 1024].as_bytes()],
        )
        .unwrap();
        assert_eq!(vec_row_count(&conn), 1);
    }

    #[test]
    fn test_insert_and_list_pending_chunks() {
        let conn = setup_db();
        let m = make_mail("m1", "<m1@ex.com>", "S", "2026-07-17T10:00:00");
        mails::insert_mail(&conn, &m).unwrap();
        insert_chunks(
            &conn,
            "m1",
            &["件名: S\nチャンク1".into(), "件名: S\nチャンク2".into()],
        )
        .unwrap();

        let pending = pending_chunks(&conn, 10).unwrap();
        assert_eq!(pending.len(), 2);
        assert_eq!(pending[0].content, "件名: S\nチャンク1");
        assert_eq!(pending_totals(&conn).unwrap().1, 2);
    }

    #[test]
    fn test_mails_without_chunks_excludes_chunked() {
        let conn = setup_db();
        let m1 = make_mail("m1", "<m1@ex.com>", "S1", "2026-07-17T10:00:00");
        let m2 = make_mail("m2", "<m2@ex.com>", "S2", "2026-07-17T11:00:00");
        mails::insert_mail(&conn, &m1).unwrap();
        mails::insert_mail(&conn, &m2).unwrap();
        insert_chunks(&conn, "m1", &["c".into()]).unwrap();

        let todo = mails_without_chunks(&conn, 10).unwrap();
        assert_eq!(todo.len(), 1);
        assert_eq!(todo[0].0, "m2");
        assert_eq!(pending_totals(&conn).unwrap().0, 1);
    }

    #[test]
    fn test_store_embedding_marks_done_and_inserts_vector() {
        let conn = setup_db();
        let m = make_mail("m1", "<m1@ex.com>", "S", "2026-07-17T10:00:00");
        mails::insert_mail(&conn, &m).unwrap();
        insert_chunks(&conn, "m1", &["c1".into()]).unwrap();
        let chunk_id = pending_chunks(&conn, 1).unwrap()[0].id;

        store_embedding(&conn, chunk_id, &vec![0.5f32; 1024]).unwrap();

        assert!(
            pending_chunks(&conn, 10).unwrap().is_empty(),
            "embedded_at が立つ"
        );
        assert_eq!(vec_row_count(&conn), 1);
    }

    #[test]
    fn test_store_embedding_rolls_back_atomically() {
        let conn = setup_db();
        let m = make_mail("m1", "<m1@ex.com>", "S", "2026-07-17T10:00:00");
        mails::insert_mail(&conn, &m).unwrap();
        insert_chunks(&conn, "m1", &["c1".into()]).unwrap();
        let chunk_id = pending_chunks(&conn, 1).unwrap()[0].id;
        // 次元不一致の埋め込みは vec0 が拒否する → embedded_at も立たないこと
        assert!(store_embedding(&conn, chunk_id, &[0.5f32; 4]).is_err());
        assert_eq!(pending_chunks(&conn, 10).unwrap().len(), 1, "キューに残る");
    }

    #[test]
    fn test_delete_mail_removes_vectors_and_chunks() {
        let conn = setup_db();
        let m = make_mail("m1", "<m1@ex.com>", "S", "2026-07-17T10:00:00");
        mails::insert_mail(&conn, &m).unwrap();
        insert_chunks(&conn, "m1", &["c1".into()]).unwrap();
        let chunk_id = pending_chunks(&conn, 1).unwrap()[0].id;
        store_embedding(&conn, chunk_id, &vec![0.5f32; 1024]).unwrap();

        mails::delete_mail(&conn, "m1").unwrap();

        assert_eq!(vec_row_count(&conn), 0, "vec_chunks も消える");
        let chunk_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM mail_chunks", [], |r| r.get(0))
            .unwrap();
        assert_eq!(chunk_count, 0, "mail_chunks は CASCADE で消える");
    }

    #[test]
    fn test_store_embedding_skips_orphan_when_chunk_deleted_mid_embed() {
        // メール（と mail_chunks 行）が embed の HTTP 呼び出し中に削除された場合を再現。
        // UPDATE が 0 行にマッチするので vec_chunks への INSERT はスキップされ、
        // ベクトルの孤児化を防ぐこと。
        let conn = setup_db();
        let m = make_mail("m1", "<m1@ex.com>", "S", "2026-07-17T10:00:00");
        mails::insert_mail(&conn, &m).unwrap();
        insert_chunks(&conn, "m1", &["c1".into()]).unwrap();
        let chunk_id = pending_chunks(&conn, 1).unwrap()[0].id;

        conn.execute("DELETE FROM mail_chunks WHERE id = ?1", params![chunk_id])
            .unwrap();

        let result = store_embedding(&conn, chunk_id, &vec![0.5f32; 1024]);

        assert!(result.is_ok(), "チャンク消滅は正常系（何もしない）");
        assert_eq!(
            vec_row_count(&conn),
            0,
            "vec_chunks に孤児ベクトルを残さない"
        );
    }

    #[test]
    fn test_delete_account_removes_vectors() {
        let conn = setup_db();
        let m = make_mail("m1", "<m1@ex.com>", "S", "2026-07-17T10:00:00");
        mails::insert_mail(&conn, &m).unwrap();
        insert_chunks(&conn, "m1", &["c1".into()]).unwrap();
        let chunk_id = pending_chunks(&conn, 1).unwrap()[0].id;
        store_embedding(&conn, chunk_id, &vec![0.5f32; 1024]).unwrap();

        accounts::delete_account(&conn, "acc1").unwrap();
        assert_eq!(vec_row_count(&conn), 0);
    }
}
