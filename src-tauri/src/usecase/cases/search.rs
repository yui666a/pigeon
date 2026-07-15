use serde::Deserialize;

use crate::context::Ctx;
use crate::db::search;
use crate::error::AppError;
use crate::models::mail::SearchResult;
use crate::usecase::{Registry, Risk, UseCase};

/// `search_mails` UseCase の入力。
#[derive(Deserialize)]
pub struct SearchMailsInput {
    pub account_id: String,
    pub query: String,
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

    fn risk(&self, _input: &Self::Input) -> Risk {
        Risk::Read
    }

    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        ctx.with_conn(|conn| search::search_mails(conn, &input.account_id, &input.query, 100))
    }
}

/// read 系 UseCase をレジストリにまとめて登録する。
/// 水平展開（他の read 系コマンドの UseCase 化）はここに足していく。
pub fn register_read_cases(registry: &mut Registry) {
    registry.register(SearchMailsUseCase);
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use serde_json::json;

    use super::*;
    use crate::classifier::service::{ClassifyBatches, PendingClassifications};
    use crate::context::Ctx;
    use crate::state::{DbState, SyncLocks};
    use crate::test_helpers::{insert_test_mail, setup_db};
    use crate::usecase::{dispatch, Registry, Risk, UseCase};

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
        let uc = SearchMailsUseCase;
        let input = SearchMailsInput {
            account_id: "acc1".into(),
            query: "hello".into(),
        };
        assert_eq!(uc.risk(&input), Risk::Read);
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
}
