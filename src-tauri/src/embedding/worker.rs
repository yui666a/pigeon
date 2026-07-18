//! 埋め込みキューの消化ワーカー。
//! 「未チャンク化メールをチャンク化 → 未埋め込みチャンクをバッチ埋め込み」を
//! キューが空になるまで繰り返す。Ollama 停止中は静かに打ち切り、次回の
//! パス（次の同期後 or 次回起動時）で自然に再開する。
//! DB ロックは with_conn 単位で取得・解放し、await をまたいで保持しない。
//! 接続エラー以外のエラー（次元不一致等）はパス全体を Err で打ち切る。
//! 同じチャンクが恒常的に失敗するとキューが進まなくなる制限がある
//! （「既知の制限」参照。v1 はモデル・次元が固定のため許容）。

use tauri::{AppHandle, Manager};

use crate::db::chunks;
use crate::embedding::Embedder;
use crate::error::AppError;
use crate::mail_chunker::chunk_mail;
use crate::state::{DbState, EmbeddingRunGuard};

const CHUNKING_BATCH: u32 = 100;
const EMBED_BATCH: u32 = 16;

pub(crate) fn build_embed_inputs(doc_prefix: &str, contents: &[String]) -> Vec<String> {
    contents
        .iter()
        .map(|c| format!("{doc_prefix}{c}"))
        .collect()
}

pub async fn run_embedding_pass(
    db: &DbState,
    embedder: &dyn Embedder,
    doc_prefix: &str,
    on_progress: &mut (dyn FnMut(u64, u64) + Send),
) -> Result<u64, AppError> {
    // 1. チャンク化: 未チャンク化メールが尽きるまで
    loop {
        let batch = db.with_conn(|conn| chunks::mails_without_chunks(conn, CHUNKING_BATCH))?;
        if batch.is_empty() {
            break;
        }
        db.with_conn(|conn| {
            for (mail_id, subject, body) in &batch {
                let pieces = chunk_mail(subject, body.as_deref());
                chunks::insert_chunks(conn, mail_id, &pieces)?;
            }
            Ok(())
        })?;
    }

    // 2. 埋め込み: キューが尽きるか接続エラーまで
    let total = db.with_conn(chunks::pending_totals)?.1;
    let mut done: u64 = 0;
    loop {
        let batch = db.with_conn(|conn| chunks::pending_chunks(conn, EMBED_BATCH))?;
        if batch.is_empty() {
            break;
        }
        let contents: Vec<String> = batch.iter().map(|c| c.content.clone()).collect();
        let inputs = build_embed_inputs(doc_prefix, &contents);
        // embed の await は with_conn（＝ロック）の外で行う
        let embeddings = match embedder.embed(&inputs).await {
            Ok(e) => e,
            // 接続エラーは「今は埋め込めない」だけ。キューに残して静かに終了
            Err(AppError::OllamaConnection(_)) => break,
            Err(e) => return Err(e),
        };
        db.with_conn(|conn| {
            for (chunk, embedding) in batch.iter().zip(embeddings.iter()) {
                chunks::store_embedding(conn, chunk.id, embedding)?;
            }
            Ok(())
        })?;
        done += batch.len() as u64;
        on_progress(done, total);
    }
    Ok(done)
}

/// 埋め込みキュー消化パスを1回 spawn する共通処理（起動時パス／同期後パス共用）。
/// 多重起動は EmbeddingRunGuard で防ぐ（両呼び出し元は排他）。
/// Ollama 接続エラーは run_embedding_pass 内で Ok 打ち切りになるため、
/// ここではエラーを飲み込んで eprintln! するだけで良い。
/// guard.finish() は try_begin 成功後の全ての退出経路で必ず呼ぶ。
pub fn spawn_embedding_pass(
    app: &AppHandle,
    mut on_progress: impl FnMut(u64, u64) + Send + 'static,
) {
    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let guard = app_handle.state::<EmbeddingRunGuard>();
        if !guard.try_begin() {
            return;
        }
        let db = app_handle.state::<DbState>();
        let (embedder, doc_prefix) = match db.with_conn(|conn| {
            let embedder = crate::embedding::OllamaEmbedder::from_settings(conn)?;
            let doc_prefix =
                crate::db::settings::get_or_default(conn, "embedding_document_prefix", "")?;
            Ok((embedder, doc_prefix))
        }) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[warn] embedding pass: setup failed: {}", e);
                guard.finish();
                return;
            }
        };
        let result = run_embedding_pass(&db, &embedder, &doc_prefix, &mut |done, total| {
            on_progress(done, total)
        })
        .await;
        if let Err(e) = result {
            eprintln!("[warn] embedding pass failed: {}", e);
        }
        guard.finish();
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{chunks, mails};
    use crate::embedding::fake::FakeEmbedder;
    use crate::state::DbState;
    use crate::test_helpers::{make_mail, setup_db};

    fn db() -> DbState {
        DbState(std::sync::Mutex::new(setup_db()))
    }

    #[tokio::test]
    async fn test_pass_chunks_and_embeds_all_mails() {
        let db = db();
        {
            let conn = db.0.lock().unwrap();
            let mut m = make_mail("m1", "<m1@ex.com>", "照明の件", "2026-07-17T10:00:00");
            m.body_text = Some("仕込み図を送ります".into());
            mails::insert_mail(&conn, &m).unwrap();
        }
        let embedder = FakeEmbedder {
            dims: 1024,
            fail_always: false,
        };
        let mut calls = Vec::new();
        let done = run_embedding_pass(&db, &embedder, "", &mut |d, t| calls.push((d, t)))
            .await
            .unwrap();
        assert_eq!(done, 1);
        let conn = db.0.lock().unwrap();
        assert_eq!(chunks::pending_totals(&conn).unwrap(), (0, 0));
        assert!(!calls.is_empty(), "進捗コールバックが呼ばれる");
    }

    #[tokio::test]
    async fn test_pass_leaves_queue_when_embedder_down() {
        let db = db();
        {
            let conn = db.0.lock().unwrap();
            mails::insert_mail(
                &conn,
                &make_mail("m1", "<m1@ex.com>", "S", "2026-07-17T10:00:00"),
            )
            .unwrap();
        }
        let embedder = FakeEmbedder {
            dims: 1024,
            fail_always: true,
        };
        let done = run_embedding_pass(&db, &embedder, "", &mut |_, _| {})
            .await
            .unwrap();
        assert_eq!(done, 0, "接続エラーはOkで打ち切り");
        let conn = db.0.lock().unwrap();
        let (_, unembedded) = chunks::pending_totals(&conn).unwrap();
        assert!(unembedded >= 1, "チャンク化は済みキューに残る");
    }

    #[tokio::test]
    async fn test_pass_applies_document_prefix() {
        // doc_prefix はチャンク本文の前に付けて埋め込み入力にする
        // （FakeEmbedder は入力文字列で決まるので、prefix 有無でベクトルが変わること
        //  を直接検証するのは難しい。ここでは build_inputs 純関数を切り出して検証）
        let inputs = build_embed_inputs("検索文書: ", &["件名: S\n本文".to_string()]);
        assert_eq!(inputs, vec!["検索文書: 件名: S\n本文".to_string()]);
    }
}
