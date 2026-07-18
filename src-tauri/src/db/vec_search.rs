//! セマンティック検索の DB 層。KNN（vec_chunks）→ mail_id 集約 → SearchResult。
//! 戻り型を既存の文字列検索（db::search）と同じ SearchResult に揃えることで、
//! フロントエンドは検索モードによらず同じ表示コードを使える。

use crate::db::mails::{row_to_mail, MAIL_COLUMNS_PREFIXED, MAIL_COLUMN_COUNT};
use crate::error::AppError;
use crate::models::mail::SearchResult;
use rusqlite::{params, Connection};
use zerocopy::IntoBytes;

/// KNN の k はメール集約で目減りするため limit より広めに取る。
/// account_id フィルタは KNN の後段で適用されるため、複数アカウント環境では
/// 上位 k を他アカウントが占有すると取りこぼしが起きる（「既知の制限」参照）
const KNN_FACTOR: u32 = 4;
const KNN_MAX: u32 = 200;
const SNIPPET_MAX_CHARS: usize = 120;

pub struct ChunkHit {
    pub chunk_id: i64,
    pub mail_id: String,
    pub content: String,
    pub distance: f64,
}

/// RAG-ready の内部層: クエリベクトルに近いチャンクを距離昇順で返す。
/// 将来の RAG（要約・質問応答）はこの関数を入口にする（設計書「検索 API の層分け」）。
pub fn search_chunks(
    conn: &Connection,
    query_embedding: &[f32],
    k: u32,
) -> Result<Vec<ChunkHit>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT c.id, c.mail_id, c.content, knn.distance
         FROM (SELECT chunk_id, distance FROM vec_chunks
               WHERE embedding MATCH ?1 AND k = ?2
               ORDER BY distance) knn
         JOIN mail_chunks c ON c.id = knn.chunk_id
         ORDER BY knn.distance",
    )?;
    let hits = stmt
        .query_map(params![query_embedding.as_bytes(), k], |row| {
            Ok(ChunkHit {
                chunk_id: row.get(0)?,
                mail_id: row.get(1)?,
                content: row.get(2)?,
                distance: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(hits)
}

pub fn search_mails_semantic(
    conn: &Connection,
    account_id: &str,
    query_embedding: &[f32],
    limit: u32,
) -> Result<Vec<SearchResult>, AppError> {
    let k = (limit * KNN_FACTOR).min(KNN_MAX);
    let mut stmt = conn.prepare(&format!(
        "SELECT {}, p.id, p.name, c.content, knn.distance
         FROM (SELECT chunk_id, distance FROM vec_chunks
               WHERE embedding MATCH ?1 AND k = ?2
               ORDER BY distance) knn
         JOIN mail_chunks c ON c.id = knn.chunk_id
         JOIN mails m ON m.id = c.mail_id
         LEFT JOIN mail_project_assignments mpa ON m.id = mpa.mail_id
         LEFT JOIN projects p ON mpa.project_id = p.id
         WHERE m.account_id = ?3
         ORDER BY knn.distance",
        *MAIL_COLUMNS_PREFIXED
    ))?;

    let rows = stmt
        .query_map(params![query_embedding.as_bytes(), k, account_id], |row| {
            let mail = row_to_mail(row)?;
            let project_id: Option<String> = row.get(MAIL_COLUMN_COUNT)?;
            let project_name: Option<String> = row.get(MAIL_COLUMN_COUNT + 1)?;
            let content: String = row.get(MAIL_COLUMN_COUNT + 2)?;
            let distance: f64 = row.get(MAIL_COLUMN_COUNT + 3)?;
            Ok((mail, project_id, project_name, content, distance))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    // distance 昇順で来るので、初出の mail_id だけ採用すれば「メールごとの
    // ベストチャンク」になる。複数案件に割り当てられたメールは LEFT JOIN で
    // 行が増え、最初の行の案件が採用される（既存 db::search::search_mails と同挙動）
    let mut seen = std::collections::HashSet::new();
    let mut results = Vec::new();
    for (mail, project_id, project_name, content, _distance) in rows {
        if !seen.insert(mail.id.clone()) {
            continue;
        }
        let snippet = chunk_snippet(&content);
        results.push(SearchResult {
            mail,
            project_id,
            project_name,
            snippet,
        });
        if results.len() as u32 >= limit {
            break;
        }
    }
    Ok(results)
}

/// チャンク本文からスニペットを作る。`件名: …\n` プレフィックスを除去し、
/// 先頭 SNIPPET_MAX_CHARS 文字に切り詰める（セマンティック一致のため
/// `<b>` ハイライトは付けない）
fn chunk_snippet(content: &str) -> String {
    let body = match content.split_once('\n') {
        Some((first, rest)) if first.starts_with("件名: ") => rest,
        _ => content,
    };
    let mut s: String = body.chars().take(SNIPPET_MAX_CHARS).collect();
    if body.chars().count() > SNIPPET_MAX_CHARS {
        s.push_str("...");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{chunks, mails};
    use crate::test_helpers::{make_mail, setup_db};

    /// 単純な直交ベクトルで「近いチャンクを持つメールが上位」を検証する
    fn axis_vec(axis: usize) -> Vec<f32> {
        let mut v = vec![0.0f32; 1024];
        v[axis] = 1.0;
        v
    }

    fn insert_mail_with_embedded_chunk(
        conn: &rusqlite::Connection,
        mail_id: &str,
        subject: &str,
        content: &str,
        axis: usize,
    ) {
        let m = make_mail(
            mail_id,
            &format!("<{mail_id}@ex.com>"),
            subject,
            "2026-07-17T10:00:00",
        );
        mails::insert_mail(conn, &m).unwrap();
        chunks::insert_chunks(conn, mail_id, &[content.to_string()]).unwrap();
        let id = chunks::pending_chunks(conn, 100)
            .unwrap()
            .last()
            .unwrap()
            .id;
        chunks::store_embedding(conn, id, &axis_vec(axis)).unwrap();
    }

    #[test]
    fn test_semantic_search_ranks_by_distance() {
        let conn = setup_db();
        insert_mail_with_embedded_chunk(&conn, "m1", "照明", "件名: 照明\n灯体の件", 0);
        insert_mail_with_embedded_chunk(&conn, "m2", "音響", "件名: 音響\nスピーカー", 1);

        let results = search_mails_semantic(&conn, "acc1", &axis_vec(0), 10).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].mail.id, "m1", "クエリに近い軸のメールが1位");
    }

    #[test]
    fn test_semantic_search_groups_chunks_per_mail() {
        let conn = setup_db();
        let m = make_mail("m1", "<m1@ex.com>", "S", "2026-07-17T10:00:00");
        mails::insert_mail(&conn, &m).unwrap();
        chunks::insert_chunks(&conn, "m1", &["c1".into(), "c2".into()]).unwrap();
        for c in chunks::pending_chunks(&conn, 10).unwrap() {
            chunks::store_embedding(&conn, c.id, &axis_vec(0)).unwrap();
        }
        let results = search_mails_semantic(&conn, "acc1", &axis_vec(0), 10).unwrap();
        assert_eq!(results.len(), 1, "同一メールの複数チャンクは1件に集約");
    }

    #[test]
    fn test_semantic_search_scoped_to_account() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO accounts (id, name, email, imap_host, smtp_host, auth_type)
             VALUES ('acc2', 'O', 'o@example.com', 'i', 's', 'plain')",
            [],
        )
        .unwrap();
        insert_mail_with_embedded_chunk(&conn, "m1", "S1", "c", 0);
        let mut m2 = make_mail("m2", "<m2@ex.com>", "S2", "2026-07-17T11:00:00");
        m2.account_id = "acc2".into();
        mails::insert_mail(&conn, &m2).unwrap();
        chunks::insert_chunks(&conn, "m2", &["c".into()]).unwrap();
        let id = chunks::pending_chunks(&conn, 10).unwrap()[0].id;
        chunks::store_embedding(&conn, id, &axis_vec(0)).unwrap();

        let results = search_mails_semantic(&conn, "acc1", &axis_vec(0), 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].mail.account_id, "acc1");
    }

    #[test]
    fn test_semantic_snippet_strips_subject_prefix() {
        let conn = setup_db();
        insert_mail_with_embedded_chunk(&conn, "m1", "照明", "件名: 照明\n灯体を3台追加します", 0);
        let results = search_mails_semantic(&conn, "acc1", &axis_vec(0), 10).unwrap();
        assert_eq!(results[0].snippet, "灯体を3台追加します");
    }

    #[test]
    fn test_semantic_search_empty_index_returns_empty() {
        let conn = setup_db();
        let results = search_mails_semantic(&conn, "acc1", &axis_vec(0), 10).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_search_chunks_returns_chunk_hits_in_distance_order() {
        // RAG-ready 内部層: チャンク単位の結果が距離昇順で返る
        let conn = setup_db();
        insert_mail_with_embedded_chunk(&conn, "m1", "照明", "件名: 照明\n灯体の件", 0);
        insert_mail_with_embedded_chunk(&conn, "m2", "音響", "件名: 音響\nスピーカー", 1);

        let hits = search_chunks(&conn, &axis_vec(0), 10).unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].mail_id, "m1");
        assert!(hits[0].distance <= hits[1].distance);
        assert!(hits[0].content.contains("灯体"));
    }
}
