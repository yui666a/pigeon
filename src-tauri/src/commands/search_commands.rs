use tauri::State;

use crate::classifier::service::{ClassifyBatches, PendingClassifications};
use crate::context::Ctx;
use crate::embedding::{Embedder, OllamaEmbedder};
use crate::error::AppError;
use crate::models::mail::SearchResult;
use crate::state::{DbState, SecureStoreState, SyncLocks};
use crate::usecase::{dispatch, Registry};

/// UI driver の全文検索。dispatch バス経由（ADR 0004 D1: 特権的な裏口を作らない）。
#[tauri::command]
pub async fn search_mails(
    registry: State<'_, Registry>,
    db: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    pending: State<'_, PendingClassifications>,
    batches: State<'_, ClassifyBatches>,
    sync_locks: State<'_, SyncLocks>,
    account_id: String,
    query: String,
) -> Result<Vec<SearchResult>, AppError> {
    let ctx = Ctx::new(&db, &secure_store, &pending, &batches, &sync_locks);
    let out = dispatch(
        &registry,
        "search_mails",
        serde_json::json!({ "account_id": account_id, "query": query }),
        &ctx,
    )
    .await?;
    serde_json::from_value(out)
        .map_err(|e| AppError::Validation(format!("unexpected search output: {e}")))
}

/// UI driver のセマンティック検索。クエリの埋め込み生成（async HTTP）は
/// dispatch の外、command 側で行う。現行の dispatch は同期 run のため、
/// async な埋め込み呼び出しをそのまま UseCase::run に置くことができない。
/// dispatch に渡すのは生成済みのベクトルのみで、DB 読み（KNN 検索）は
/// 必ずバス経由という ADR 0004 の境界はここでも保たれる。
#[tauri::command]
pub async fn semantic_search(
    registry: State<'_, Registry>,
    db: State<'_, DbState>,
    secure_store: State<'_, SecureStoreState>,
    pending: State<'_, PendingClassifications>,
    batches: State<'_, ClassifyBatches>,
    sync_locks: State<'_, SyncLocks>,
    account_id: String,
    query: String,
) -> Result<Vec<SearchResult>, String> {
    let (embedder, prefix) = db
        .with_conn(|conn| {
            let embedder = OllamaEmbedder::from_settings(conn)?;
            let prefix = crate::db::settings::get_or_default(conn, "embedding_query_prefix", "")?;
            Ok::<_, AppError>((embedder, prefix))
        })
        .map_err(|e| e.to_string())?;

    let mut embeddings = embedder
        .embed(&[format!("{prefix}{query}")])
        .await
        .map_err(|e| e.to_string())?;
    let embedding = if embeddings.is_empty() {
        Vec::new()
    } else {
        embeddings.remove(0)
    };

    let ctx = Ctx::new(&db, &secure_store, &pending, &batches, &sync_locks);
    let out = dispatch(
        &registry,
        "semantic_search_mails",
        serde_json::json!({ "account_id": account_id, "embedding": embedding }),
        &ctx,
    )
    .await
    .map_err(|e| e.to_string())?;
    serde_json::from_value(out).map_err(|e| format!("unexpected search output: {e}"))
}
