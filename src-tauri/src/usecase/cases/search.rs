use serde::Deserialize;

use crate::context::Ctx;
use crate::db::{search, vec_search};
use crate::error::AppError;
use crate::models::mail::SearchResult;
use crate::usecase::{Registry, Risk, UseCase};

/// `search_mails` UseCase の入力。
#[derive(Deserialize)]
pub struct SearchMailsInput {
    pub account_id: String,
    pub query: String,
    #[serde(default)]
    pub project_id: Option<String>,
}

/// 全文検索の read 系 UseCase（バスに載せる最初の実例）。
pub struct SearchMailsUseCase;

#[async_trait::async_trait]
impl UseCase for SearchMailsUseCase {
    type Input = SearchMailsInput;
    type Output = Vec<SearchResult>;

    fn name(&self) -> &'static str {
        "search_mails"
    }

    fn risk(&self, _input: &Self::Input, _ctx: &Ctx) -> Result<Risk, AppError> {
        Ok(Risk::Read)
    }

    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        ctx.with_conn(|conn| {
            search::search_mails(
                conn,
                &input.account_id,
                &input.query,
                input.project_id.as_deref(),
                100,
            )
        })
    }
}

/// `semantic_search_mails` UseCase の入力。埋め込み生成（async HTTP）は
/// command 側で終えてあり、ここではベクトルを受け取るだけ（dispatch は同期 run のため）。
#[derive(Deserialize)]
pub struct SemanticSearchInput {
    pub account_id: String,
    pub embedding: Vec<f32>,
    #[serde(default)]
    pub project_id: Option<String>,
}

/// セマンティック検索の read 系 UseCase。DB 読みは必ずバス経由という
/// ADR 0004 の境界を保ちつつ、クエリ埋め込み生成だけは command 側に置く。
pub struct SemanticSearchUseCase;

#[async_trait::async_trait]
impl UseCase for SemanticSearchUseCase {
    type Input = SemanticSearchInput;
    type Output = Vec<SearchResult>;

    fn name(&self) -> &'static str {
        "semantic_search_mails"
    }

    fn risk(&self, _input: &Self::Input, _ctx: &Ctx) -> Result<Risk, AppError> {
        Ok(Risk::Read)
    }

    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        ctx.with_conn(|conn| {
            vec_search::search_mails_semantic(
                conn,
                &input.account_id,
                &input.embedding,
                input.project_id.as_deref(),
                100,
            )
        })
    }
}

/// read 系 UseCase をレジストリにまとめて登録する。
/// 水平展開（他の read 系コマンドの UseCase 化）はここに足していく。
pub fn register_read_cases(registry: &mut Registry) {
    registry.register(SearchMailsUseCase);
    registry.register(SemanticSearchUseCase);
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use serde_json::json;

    use super::*;
    use crate::classifier::service::{ClassifyBatches, PendingClassifications};
    use crate::context::Ctx;
    use crate::db::chunks;
    use crate::state::{DbState, SyncLocks};
    use crate::test_helpers::{insert_test_mail, setup_db};
    use crate::usecase::{dispatch, Registry, Risk, UseCase};

    /// vec_search のテストと同じ考え方: 単純な直交ベクトルで
    /// 「クエリに近い軸のメールが返る」ことを検証する。
    fn axis_vec(axis: usize) -> Vec<f32> {
        let mut v = vec![0.0f32; 1024];
        v[axis] = 1.0;
        v
    }

    fn build_states() -> (DbState, PendingClassifications, ClassifyBatches, SyncLocks) {
        (
            DbState(Mutex::new(setup_db())),
            PendingClassifications::new(),
            ClassifyBatches::new(),
            SyncLocks::new(),
        )
    }

    #[test]
    fn test_search_usecase_declares_read_risk() {
        let (db, pending, batches, locks) = build_states();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks);
        let uc = SearchMailsUseCase;
        let input = SearchMailsInput {
            account_id: "acc1".into(),
            query: "hello".into(),
            project_id: None,
        };
        assert_eq!(uc.risk(&input, &ctx).unwrap(), Risk::Read);
        assert_eq!(uc.name(), "search_mails");
    }

    #[tokio::test]
    async fn test_search_via_dispatch_matches_direct_query() {
        let (db, pending, batches, locks) = build_states();
        // setup_db 済み。件名に "Report" を含むメールを1件入れる
        {
            let conn = db.0.lock().unwrap();
            insert_test_mail(&conn, "m1", "Quarterly Report");
        }
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks);

        let mut reg = Registry::new();
        register_read_cases(&mut reg);

        let out = dispatch(
            &reg,
            "search_mails",
            json!({ "account_id": "acc1", "query": "Report" }),
            &ctx,
        )
        .await
        .expect("search should dispatch");

        // 出力は Vec<SearchResult> の JSON。1件ヒットする
        let arr = out.as_array().expect("output is a JSON array");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["mail"]["id"], "m1");
    }

    #[tokio::test]
    async fn test_search_via_dispatch_empty_query_returns_empty() {
        let (db, pending, batches, locks) = build_states();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks);
        let mut reg = Registry::new();
        register_read_cases(&mut reg);

        let out = dispatch(
            &reg,
            "search_mails",
            json!({ "account_id": "acc1", "query": "" }),
            &ctx,
        )
        .await
        .expect("empty query should dispatch");
        assert_eq!(out, json!([]));
    }

    #[test]
    fn test_semantic_search_usecase_is_read_risk() {
        let (db, pending, batches, locks) = build_states();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks);
        let uc = SemanticSearchUseCase;
        let input = SemanticSearchInput {
            account_id: "acc1".into(),
            embedding: axis_vec(0),
            project_id: None,
        };
        assert_eq!(uc.risk(&input, &ctx).unwrap(), Risk::Read);
        assert_eq!(uc.name(), "semantic_search_mails");
    }

    #[tokio::test]
    async fn test_semantic_search_usecase_returns_results_via_dispatch() {
        let (db, pending, batches, locks) = build_states();
        // 実データ: メール + チャンク + axis ベクトル埋め込みを挿入する
        {
            let conn = db.0.lock().unwrap();
            insert_test_mail(&conn, "m1", "Quarterly Report");
            chunks::insert_chunks(&conn, "m1", &["件名: Quarterly Report\n内容".to_string()])
                .unwrap();
            let id = chunks::pending_chunks(&conn, 10).unwrap()[0].id;
            chunks::store_embedding(&conn, id, &axis_vec(0)).unwrap();
        }
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks);

        let mut reg = Registry::new();
        register_read_cases(&mut reg);

        let out = dispatch(
            &reg,
            "semantic_search_mails",
            json!({ "account_id": "acc1", "embedding": axis_vec(0) }),
            &ctx,
        )
        .await
        .expect("semantic search should dispatch");

        let arr = out.as_array().expect("output is a JSON array");
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["mail"]["id"], "m1");
    }
}
