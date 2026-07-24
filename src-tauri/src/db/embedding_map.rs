//! 埋め込みマップ用に vec_chunks から生ベクトルとラベルを全件読み出す。
//! 既存の vec_search は距離しか読まないため、生ベクトルの復元はここが唯一。

use crate::error::AppError;
use rusqlite::Connection;

/// bge-m3 の次元数。vec_chunks の float[1024] に対応（migrations v18）。
const EMBEDDING_DIM: usize = 1024;

pub struct MapChunkRow {
    pub mail_id: String,
    pub subject: String,
    pub project_id: Option<String>,
    pub project_name: Option<String>,
    pub project_color: Option<String>,
    pub vector: Vec<f32>,
}

/// vec_chunks.embedding の生バイト列を f32 配列へ復元する。
/// 書き込みは zerocopy::IntoBytes（chunks.rs）でリトルエンディアン。
pub fn decode_embedding(blob: &[u8]) -> Result<Vec<f32>, AppError> {
    if blob.len() % 4 != 0 {
        return Err(AppError::Validation(format!(
            "f32 境界に揃っていません: {} バイト",
            blob.len()
        )));
    }
    Ok(blob
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect())
}

/// 全チャンクの生ベクトルとラベルを読む。JOIN 構造は vec_search を踏襲。
/// 次元が EMBEDDING_DIM でない行はスキップする（再埋め込み途中など）。
pub fn load_map_chunks(conn: &Connection) -> Result<Vec<MapChunkRow>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT v.embedding, c.mail_id, m.subject, mpa.project_id, p.name, p.color
         FROM vec_chunks v
         JOIN mail_chunks c ON c.id = v.chunk_id
         JOIN mails m ON m.id = c.mail_id
         LEFT JOIN mail_project_assignments mpa ON mpa.mail_id = m.id
         LEFT JOIN projects p ON p.id = mpa.project_id
         ORDER BY c.mail_id, c.chunk_index",
    )?;
    let rows = stmt.query_map([], |row| {
        let blob: Vec<u8> = row.get(0)?;
        Ok((
            blob,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(5)?,
        ))
    })?;

    let mut result = Vec::new();
    let mut skipped = 0usize;
    for row in rows {
        let (blob, mail_id, subject, project_id, project_name, project_color) = row?;
        let vector = decode_embedding(&blob)?;
        if vector.len() != EMBEDDING_DIM {
            skipped += 1;
            continue;
        }
        result.push(MapChunkRow {
            mail_id,
            subject: subject.unwrap_or_else(|| "(件名なし)".to_string()),
            project_id,
            project_name,
            project_color,
            vector,
        });
    }
    if skipped > 0 {
        eprintln!("警告: 次元が {EMBEDDING_DIM} でない行を {skipped} 件スキップ");
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_embedding_roundtrips_le_f32() {
        let original: Vec<f32> = vec![0.5, -1.25, 3.0, 0.0];
        let mut blob = Vec::new();
        for f in &original {
            blob.extend_from_slice(&f.to_le_bytes());
        }
        assert_eq!(decode_embedding(&blob).unwrap(), original);
    }

    #[test]
    fn decode_embedding_rejects_misaligned_length() {
        // f32 は 4 バイト境界。5 バイトは壊れたデータ。
        assert!(decode_embedding(&[0u8; 5]).is_err());
    }
}
