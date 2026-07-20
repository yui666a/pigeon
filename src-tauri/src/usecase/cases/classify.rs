//! 未分類メールのバッチ分類の use case。分類器の構築と進捗送出を
//! ここに集約し、driver（GUI / CLI / MCP）に依存しない形にする。
//! 進捗は ProgressSink 経由で送るため、イベント名 "classify-progress" と
//! payload のキー（account_id / current / total / assigned_mail_id）は
//! frontend の `ClassifyProgressEvent` 型（src/types/classifier.ts）と
//! 一致させること。

use serde::Deserialize;

use crate::classifier::factory::build_classifier;
use crate::classifier::service;
use crate::classifier::service::PendingClassifications;
use crate::context::Ctx;
use crate::db::assignments;
use crate::error::AppError;
use crate::models::classifier::ClassifyBatchOutcome;
use crate::models::mail::ThreadPage;
use crate::usecase::cases::mailbox::resolve_page_window;
use crate::usecase::{Registry, Risk, UseCase};

#[derive(Deserialize, schemars::JsonSchema)]
pub struct ClassifyBatchInput {
    pub account_id: String,
}

pub struct ClassifyBatchUseCase;

/// classify-progress の payload を組み立てる。
///
/// キー名は frontend の `ClassifyProgressEvent`（src/types/classifier.ts）が
/// そのまま読むため、変更すると進捗表示と未分類一覧の即時反映が壊れる。
/// 純関数に切り出して wire contract をテストで固定している。
fn classify_progress_payload(
    account_id: &str,
    current: usize,
    total: usize,
    assigned_mail_id: Option<&str>,
) -> serde_json::Value {
    serde_json::json!({
        "account_id": account_id,
        "current": current,
        "total": total,
        "assigned_mail_id": assigned_mail_id,
    })
}

#[async_trait::async_trait]
impl UseCase for ClassifyBatchUseCase {
    type Input = ClassifyBatchInput;
    type Output = ClassifyBatchOutcome;

    fn name(&self) -> &'static str {
        "classify_batch"
    }

    fn risk(&self, _input: &Self::Input, _ctx: &Ctx) -> Result<Risk, AppError> {
        // 案件割り当ての変更は取り消せる
        Ok(Risk::Reversible)
    }

    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        let secure_store = ctx.secure_store()?;
        let classifier = ctx.with_conn(|conn| build_classifier(conn, secure_store))?;
        let account_id = input.account_id;
        service::classify_batch(
            &ctx.db().0,
            classifier.as_ref(),
            ctx.pending(),
            ctx.batches(),
            &account_id,
            |current, total, assigned_mail_id| {
                // 進捗はベストエフォート（送出失敗で分類は止めない）
                ctx.progress().emit(
                    "classify-progress",
                    &classify_progress_payload(&account_id, current, total, assigned_mail_id),
                );
            },
        )
        .await
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct GetUnclassifiedThreadsInput {
    /// 対象アカウントの ID。
    pub account_id: String,
    /// 返すスレッドの最大件数。省略時は 200、上限は 500。
    ///
    /// 上限はメールではなくスレッドに掛かる。窓に入ったスレッドは
    /// メールが揃った状態で返るため、`limit: 1` でも 5 通のスレッドは
    /// 5 通すべて含んで返る。件数の見積もりには使えない。
    pub limit: Option<usize>,
    /// 読み飛ばすスレッド数。省略時は 0。
    ///
    /// 続きを取るには前回の `limit` + `offset` を渡し、`has_more` が
    /// false になるまで繰り返す。総件数は返さない。
    pub offset: Option<usize>,
}

/// 未分類メールをスレッド単位で1ページ分返す。
///
/// 取得の前にスレッド追従の自動分類（`auto_follow_threads`）を行う。
/// 同一スレッドの既存メールが単一の案件に割り当て済みなら、後から届いた
/// 返信等の未分類メールをその案件へ自動追従させる
/// （設計: docs/archive/specs/2026-07-13-thread-follow-classify-design.md）。
///
/// 読み取り系の名前だが Risk は Read ではない。追従による割り当ての確定
/// （取り消し可能な書き込み）を伴うため Reversible とする。
pub struct GetUnclassifiedThreadsUseCase;

#[async_trait::async_trait]
impl UseCase for GetUnclassifiedThreadsUseCase {
    type Input = GetUnclassifiedThreadsInput;
    type Output = ThreadPage;

    fn name(&self) -> &'static str {
        "get_unclassified_threads"
    }

    fn risk(&self, _input: &Self::Input, _ctx: &Ctx) -> Result<Risk, AppError> {
        // 一覧取得に見えて auto_follow_threads が割り当てを書き込む
        Ok(Risk::Reversible)
    }

    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        let (limit, offset) = resolve_page_window(input.limit, input.offset);
        let pending = ctx.pending();
        ctx.with_conn(|conn| {
            get_unclassified_thread_page(conn, pending, &input.account_id, limit, offset)
        })
    }
}

/// 未分類スレッド1ページ分の本体。追従 → 軽量メタでの窓決め → 本文読み出し。
///
/// 切り出しはスレッド単位（ADR 0006 決定5）。メール単位で切ると同じスレッドの
/// 一部だけが窓に入り、mail_count や参加者一覧が実データと食い違う。
pub(crate) fn get_unclassified_thread_page(
    conn: &rusqlite::Connection,
    pending: &PendingClassifications,
    account_id: &str,
    limit: usize,
    offset: usize,
) -> Result<ThreadPage, AppError> {
    let followed = assignments::auto_follow_threads(conn, account_id)?;
    // スレッド追従で割り当てが確定したメールの提案は不要になる
    for mail_id in &followed {
        pending.remove(mail_id)?;
    }

    // 1段目: 本文を読まない軽量メタでスレッドを構成し、窓をスレッド境界で切る
    let metas = assignments::get_unclassified_thread_metas(conn, account_id)?;
    let groups = crate::db::mails::group_mail_ids_into_threads(&metas);
    let total = groups.len();
    let page_ids: Vec<String> = groups
        .into_iter()
        .skip(offset)
        .take(limit)
        .flatten()
        .collect();
    let has_more = total > offset.saturating_add(limit);

    // 2段目: 窓に入ったスレッドのメールだけを本文込みで読む
    let mails = assignments::get_unclassified_mails_by_ids(conn, &page_ids)?;
    Ok(ThreadPage {
        threads: crate::db::mails::build_threads(&mails),
        has_more,
    })
}

pub fn register_classify_cases(registry: &mut Registry) {
    registry.register(ClassifyBatchUseCase);
    registry.register(GetUnclassifiedThreadsUseCase);
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::classifier::service::{ClassifyBatches, PendingClassifications};
    use crate::state::{DbState, SyncLocks};
    use crate::test_helpers::setup_db;

    fn build_states() -> (DbState, PendingClassifications, ClassifyBatches, SyncLocks) {
        (
            DbState(Mutex::new(setup_db())),
            PendingClassifications::new(),
            ClassifyBatches::new(),
            SyncLocks::new(),
        )
    }

    /// 追従で割り当てが確定したメールの Create 提案も除去される
    /// （commands::classify_commands から本体と一緒に移設）
    #[test]
    fn test_unclassified_page_removes_followed_pending() {
        let conn = setup_db();
        let pending = PendingClassifications::new();
        let m1 =
            crate::test_helpers::make_mail("m1", "<m1@ex.com>", "Re: Test", "2026-07-12T10:00:00");
        let mut m2 =
            crate::test_helpers::make_mail("m2", "<m2@ex.com>", "Re: Test", "2026-07-12T11:00:00");
        m2.in_reply_to = Some("<m1@ex.com>".into());
        crate::db::mails::insert_mail(&conn, &m1).unwrap();
        crate::db::mails::insert_mail(&conn, &m2).unwrap();
        crate::db::projects::insert_project_with_id(&conn, "p1", "acc1", "Proj", None, None, None)
            .unwrap();
        assignments::assign_mail(&conn, "m1", "p1", "user", Some(1.0)).unwrap();
        pending
            .insert(
                "m2".into(),
                crate::models::classifier::ClassifyResult {
                    action: crate::models::classifier::ClassifyAction::Create {
                        project_name: "Suggested".into(),
                        description: "desc".into(),
                        parent_project_id: None,
                    },
                    confidence: 0.8,
                    reason: "test".into(),
                },
            )
            .unwrap();

        let page = get_unclassified_thread_page(&conn, &pending, "acc1", 200, 0).unwrap();

        assert!(
            page.threads.is_empty(),
            "m2 は追従割り当てされ一覧から消える"
        );
        assert!(!page.has_more);
        assert!(
            !pending.contains("m2").unwrap(),
            "追従で確定したメールの提案も除去される"
        );
    }

    /// 未分類一覧もスレッド単位で切る（ADR 0006 決定5）。
    /// スレッドが分断されないこと・ページ間で重複しないこと。
    #[test]
    fn test_unclassified_page_pages_by_thread() {
        let conn = setup_db();
        let pending = PendingClassifications::new();

        // 独立3スレッド + 2通スレッド1件（計4スレッド、5通）
        for i in 0..3 {
            let m = crate::test_helpers::make_mail(
                &format!("s{i}"),
                &format!("<s{i}@ex.com>"),
                &format!("単独 {i}"),
                &format!("2026-07-12T0{i}:00:00"),
            );
            crate::db::mails::insert_mail(&conn, &m).unwrap();
        }
        let root =
            crate::test_helpers::make_mail("r1", "<r1@ex.com>", "会話", "2026-07-12T10:00:00");
        let mut reply =
            crate::test_helpers::make_mail("r2", "<r2@ex.com>", "Re: 会話", "2026-07-12T11:00:00");
        reply.in_reply_to = Some("<r1@ex.com>".into());
        crate::db::mails::insert_mail(&conn, &root).unwrap();
        crate::db::mails::insert_mail(&conn, &reply).unwrap();

        let first = get_unclassified_thread_page(&conn, &pending, "acc1", 1, 0).unwrap();
        assert_eq!(first.threads.len(), 1);
        assert!(first.has_more);
        assert_eq!(
            first.threads[0].mails.len(),
            2,
            "最新スレッドは2通揃って返る（メール単位LIMITなら1通に欠ける）"
        );

        let rest = get_unclassified_thread_page(&conn, &pending, "acc1", 10, 1).unwrap();
        assert_eq!(rest.threads.len(), 3, "残りの単独スレッド");
        assert!(!rest.has_more);

        // ページ間でメールが重複しない
        let mut ids: Vec<String> = first
            .threads
            .iter()
            .chain(rest.threads.iter())
            .flat_map(|t| t.mails.iter().map(|m| m.id.clone()))
            .collect();
        let total = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), total);
        assert_eq!(total, 5, "全メールを網羅する");
    }

    /// 一覧取得に見えるが追従で書き込むため Read ではない
    #[tokio::test]
    async fn test_unclassified_threads_risk_is_reversible() {
        let (db, pending, batches, sync_locks) = build_states();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &sync_locks);
        let input = GetUnclassifiedThreadsInput {
            account_id: "acc1".into(),
            limit: None,
            offset: None,
        };
        assert_eq!(
            GetUnclassifiedThreadsUseCase.risk(&input, &ctx).unwrap(),
            Risk::Reversible
        );
    }

    #[tokio::test]
    async fn test_classify_batch_risk_is_reversible() {
        let (db, pending, batches, sync_locks) = build_states();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &sync_locks);
        let input = ClassifyBatchInput {
            account_id: "acct-1".into(),
        };
        assert_eq!(
            ClassifyBatchUseCase.risk(&input, &ctx).expect("risk"),
            Risk::Reversible
        );
    }

    #[test]
    fn test_progress_payload_matches_frontend_contract() {
        // キー名は src/types/classifier.ts の ClassifyProgressEvent と一致させる。
        // 変更すると進捗表示と未分類一覧の即時反映が無言で壊れる
        let payload = classify_progress_payload("acct-1", 2, 5, Some("mail-42"));
        assert_eq!(
            payload,
            serde_json::json!({
                "account_id": "acct-1",
                "current": 2,
                "total": 5,
                "assigned_mail_id": "mail-42",
            })
        );
    }

    #[test]
    fn test_progress_payload_assigned_mail_id_is_null_when_absent() {
        // フロントは null を「確定割り当てなし」と読む（型は string | null）
        let payload = classify_progress_payload("acct-1", 1, 3, None);
        assert!(payload["assigned_mail_id"].is_null());
    }

    #[test]
    fn test_registered_in_registry() {
        let mut registry = Registry::new();
        register_classify_cases(&mut registry);
        assert!(registry.names().contains(&"classify_batch"));
    }
}
