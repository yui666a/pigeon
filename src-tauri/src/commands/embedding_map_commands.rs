//! 埋め込みマップの座標算出 command。
//! 生ベクトル読み出し（db::embedding_map）→ メール単位 centroid 集約 →
//! べき乗法 PCA（pca::project_2d）を束ね、2D 座標 + ラベルを返す。

use crate::db::embedding_map::{load_map_chunks, MapChunkRow};
use crate::error::AppError;
use crate::pca::project_2d;
use crate::state::DbState;
use tauri::State;

#[derive(serde::Serialize)]
pub struct MapPoint {
    pub x: f32,
    pub y: f32,
    pub mail_id: String,
    pub subject: String,
    pub project_id: Option<String>,
    pub project_name: Option<String>,
    pub project_color: Option<String>,
}

/// メール単位に集約した中間表現（ラベル + centroid ベクトル）。
struct MailAgg {
    mail_id: String,
    subject: String,
    project_id: Option<String>,
    project_name: Option<String>,
    project_color: Option<String>,
    vector: Vec<f32>,
}

/// 同一メールのチャンクを centroid（要素ごとの平均）へ集約する。
/// ラベルは最初のチャンクのものを採用（同一メール内で同じ）。
/// 入力の並び順（mail_id, chunk_index）を保つため、初出順を維持する。
///
/// `order` に積んだ mail_id は必ず `groups` に存在する（同じループで同時に
/// 詰めているため）が、`unwrap`/`expect` 禁止規約に従い `ok_or_else` で
/// エラーへ変換し `collect::<Result<Vec<_>, _>>()` で束ねる（理論上到達不能
/// でも、パニックで落とすのではなくエラー値として扱えるようにしておく）。
fn aggregate_by_mail(rows: Vec<MapChunkRow>) -> Result<Vec<MailAgg>, AppError> {
    use std::collections::HashMap;
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Vec<MapChunkRow>> = HashMap::new();
    for r in rows {
        if !groups.contains_key(&r.mail_id) {
            order.push(r.mail_id.clone());
        }
        groups.entry(r.mail_id.clone()).or_default().push(r);
    }

    order
        .into_iter()
        .map(|mail_id| {
            let group = groups
                .remove(&mail_id)
                .ok_or_else(|| AppError::Validation("集約の不変条件違反".to_string()))?;
            let dim = group[0].vector.len();
            let mut centroid = vec![0.0f32; dim];
            for r in &group {
                for (c, &x) in centroid.iter_mut().zip(r.vector.iter()) {
                    *c += x;
                }
            }
            let count = group.len() as f32;
            for c in centroid.iter_mut() {
                *c /= count;
            }
            let head = &group[0];
            Ok(MailAgg {
                mail_id,
                subject: head.subject.clone(),
                project_id: head.project_id.clone(),
                project_name: head.project_name.clone(),
                project_color: head.project_color.clone(),
                vector: centroid,
            })
        })
        .collect::<Result<Vec<_>, AppError>>()
}

#[tauri::command]
pub fn embedding_map_points(db: State<DbState>) -> Result<Vec<MapPoint>, AppError> {
    let rows = db.with_conn(load_map_chunks)?;
    let mails = aggregate_by_mail(rows)?;
    let vectors: Vec<Vec<f32>> = mails.iter().map(|m| m.vector.clone()).collect();
    let coords = project_2d(&vectors)?;

    let points = mails
        .into_iter()
        .zip(coords)
        .map(|(m, (x, y))| MapPoint {
            x,
            y,
            mail_id: m.mail_id,
            subject: m.subject,
            project_id: m.project_id,
            project_name: m.project_name,
            project_color: m.project_color,
        })
        .collect();
    Ok(points)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::embedding_map::MapChunkRow;

    fn row(mail_id: &str, vector: Vec<f32>) -> MapChunkRow {
        MapChunkRow {
            mail_id: mail_id.to_string(),
            subject: format!("件名{mail_id}"),
            project_id: Some("p1".to_string()),
            project_name: Some("案件A".to_string()),
            project_color: Some("#ff0000".to_string()),
            vector,
        }
    }

    #[test]
    fn aggregates_chunks_of_same_mail_by_centroid() {
        let rows = vec![row("m1", vec![1.0, 0.0]), row("m1", vec![0.0, 2.0])];
        let mails = aggregate_by_mail(rows).unwrap();
        assert_eq!(mails.len(), 1);
        // centroid は要素ごとの平均 [0.5, 1.0]
        assert_eq!(mails[0].vector, vec![0.5, 1.0]);
        assert_eq!(mails[0].mail_id, "m1");
    }

    #[test]
    fn keeps_distinct_mails_separate() {
        let rows = vec![row("m1", vec![1.0, 0.0]), row("m2", vec![0.0, 1.0])];
        let mails = aggregate_by_mail(rows).unwrap();
        assert_eq!(mails.len(), 2);
    }

    #[test]
    fn preserves_label_from_first_chunk() {
        let rows = vec![row("m1", vec![1.0, 0.0]), row("m1", vec![0.0, 2.0])];
        let mails = aggregate_by_mail(rows).unwrap();
        assert_eq!(mails[0].project_color.as_deref(), Some("#ff0000"));
    }
}
