use serde_json::Value;

use crate::context::Ctx;
use crate::db::approval_queue;
use crate::error::AppError;
use crate::usecase::gate::GateOutcome;
use crate::usecase::{gate, AuditEntry, Registry, Risk};

/// 単一の chokepoint。3 driver すべてがこの 1 関数を通る（特権的な裏口なし）。
/// lookup → risk 判定 → ゲート → 監査 → 実行 のパイプライン（ADR 0004 D5）。
///
/// - ゲートが RequireApproval を返した場合は実行せず承認キューへ積み、
///   保留を意味するエラーを返す（承認 UI と再実行は Phase 5-2）
/// - Reversible / Sensitive は実行前に監査ログへ記録する（実行失敗も試行として残る）。
///   監査の書き込み失敗は操作を止めない（fail-open。シンクが警告ログを残す）
pub async fn dispatch(
    registry: &Registry,
    name: &str,
    input: Value,
    ctx: &Ctx<'_>,
) -> Result<Value, AppError> {
    let uc = registry
        .lookup(name)
        .ok_or_else(|| AppError::Validation(format!("unknown use case: {name}")))?;

    let risk = uc.risk_json(&input, ctx)?;

    match gate::check(risk, ctx.driver()) {
        GateOutcome::Allow => {}
        GateOutcome::RequireApproval => {
            let ts = chrono::Utc::now().to_rfc3339();
            let id = ctx.with_conn(|conn| {
                approval_queue::enqueue(conn, &ts, name, &input.to_string(), ctx.driver().as_str())
            })?;
            return Err(AppError::Validation(format!(
                "approval required: '{name}' is queued for human approval (approval_queue #{id})"
            )));
        }
    }

    if risk != Risk::Read {
        let entry = AuditEntry::new(name, risk, ctx.driver(), &input);
        ctx.with_conn(|conn| {
            ctx.audit().record(conn, &entry);
            Ok(())
        })?;
    }

    uc.run_json(input, ctx).await
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    use serde::{Deserialize, Serialize};
    use serde_json::json;

    use super::*;
    use crate::classifier::service::{ClassifyBatches, PendingClassifications};
    use crate::context::Ctx;
    use crate::error::AppError;
    use crate::state::{DbState, SyncLocks};
    use crate::test_helpers::setup_db;
    use crate::usecase::{Driver, InMemoryAuditSink, Registry, Risk, UseCase};

    #[derive(Deserialize, schemars::JsonSchema)]
    struct EchoInput {
        text: String,
    }
    #[derive(Serialize)]
    struct EchoOutput {
        echoed: String,
    }
    struct EchoUseCase;
    #[async_trait::async_trait]
    impl UseCase for EchoUseCase {
        type Input = EchoInput;
        type Output = EchoOutput;
        fn name(&self) -> &'static str {
            "echo"
        }
        fn risk(&self, _input: &Self::Input, _ctx: &Ctx) -> Result<Risk, AppError> {
            Ok(Risk::Read)
        }
        async fn run(&self, input: Self::Input, _ctx: &Ctx) -> Result<Self::Output, AppError> {
            Ok(EchoOutput { echoed: input.text })
        }
    }

    #[derive(Deserialize, schemars::JsonSchema)]
    struct NoInput {}
    #[derive(Serialize)]
    struct NoOutput {}

    /// 実行されたかを外から観測できる use case（Risk は構築時指定）。
    struct ProbeUseCase {
        name: &'static str,
        risk: Risk,
        ran: Arc<AtomicBool>,
    }
    #[async_trait::async_trait]
    impl UseCase for ProbeUseCase {
        type Input = NoInput;
        type Output = NoOutput;
        fn name(&self) -> &'static str {
            self.name
        }
        fn risk(&self, _input: &Self::Input, _ctx: &Ctx) -> Result<Risk, AppError> {
            Ok(self.risk)
        }
        async fn run(&self, _input: Self::Input, _ctx: &Ctx) -> Result<Self::Output, AppError> {
            self.ran.store(true, Ordering::SeqCst);
            Ok(NoOutput {})
        }
    }

    fn build_states() -> (DbState, PendingClassifications, ClassifyBatches, SyncLocks) {
        (
            DbState(Mutex::new(setup_db())),
            PendingClassifications::new(),
            ClassifyBatches::new(),
            SyncLocks::new(),
        )
    }

    fn build_registry(risk: Risk, ran: &Arc<AtomicBool>) -> Registry {
        let mut reg = Registry::new();
        reg.register(EchoUseCase);
        reg.register(ProbeUseCase {
            name: "probe",
            risk,
            ran: Arc::clone(ran),
        });
        reg
    }

    #[tokio::test]
    async fn test_dispatch_read_usecase_succeeds_without_audit() {
        let (db, pending, batches, locks) = build_states();
        let sink = InMemoryAuditSink::new();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks).with_audit_sink(&sink);
        let reg = build_registry(Risk::Read, &Arc::new(AtomicBool::new(false)));

        let out = dispatch(&reg, "echo", json!({ "text": "hi" }), &ctx)
            .await
            .expect("read use case should dispatch");
        assert_eq!(out, json!({ "echoed": "hi" }));
        assert!(sink.entries().is_empty(), "Read は監査対象外");
    }

    #[tokio::test]
    async fn test_dispatch_unknown_name_errors() {
        let (db, pending, batches, locks) = build_states();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks);
        let reg = build_registry(Risk::Read, &Arc::new(AtomicBool::new(false)));

        let err = dispatch(&reg, "nope", json!({}), &ctx)
            .await
            .expect_err("unknown name should error");
        assert!(matches!(err, AppError::Validation(_)));
    }

    #[tokio::test]
    async fn test_dispatch_reversible_runs_and_audits() {
        let (db, pending, batches, locks) = build_states();
        let sink = InMemoryAuditSink::new();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks).with_audit_sink(&sink);
        let ran = Arc::new(AtomicBool::new(false));
        let reg = build_registry(Risk::Reversible, &ran);

        dispatch(&reg, "probe", json!({}), &ctx)
            .await
            .expect("Reversible from Ui should run");
        assert!(ran.load(Ordering::SeqCst));

        let entries = sink.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].use_case, "probe");
        assert_eq!(entries[0].risk, Risk::Reversible);
        assert_eq!(entries[0].driver, Driver::Ui);
    }

    #[tokio::test]
    async fn test_dispatch_sensitive_from_ui_runs_and_audits() {
        let (db, pending, batches, locks) = build_states();
        let sink = InMemoryAuditSink::new();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks).with_audit_sink(&sink);
        let ran = Arc::new(AtomicBool::new(false));
        let reg = build_registry(Risk::Sensitive, &ran);

        dispatch(&reg, "probe", json!({}), &ctx)
            .await
            .expect("Sensitive from Ui is pre-approved by the human click");
        assert!(ran.load(Ordering::SeqCst));
        assert_eq!(sink.entries().len(), 1);
    }

    #[tokio::test]
    async fn test_dispatch_sensitive_from_mcp_is_queued_not_run() {
        let (db, pending, batches, locks) = build_states();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks).with_driver(Driver::Mcp);
        let ran = Arc::new(AtomicBool::new(false));
        let reg = build_registry(Risk::Sensitive, &ran);

        let err = dispatch(&reg, "probe", json!({}), &ctx)
            .await
            .expect_err("Sensitive from Mcp must be held for approval");
        assert!(err.to_string().contains("approval required"), "{err}");
        assert!(!ran.load(Ordering::SeqCst), "保留された操作は実行されない");

        // 承認キューに pending で積まれている
        let pending_rows = db
            .with_conn(|conn| crate::db::approval_queue::list_pending(conn))
            .unwrap();
        assert_eq!(pending_rows.len(), 1);
        assert_eq!(pending_rows[0].use_case, "probe");
        assert_eq!(pending_rows[0].driver, "mcp");
    }

    #[tokio::test]
    async fn test_dispatch_reversible_from_agent_runs_with_audit() {
        let (db, pending, batches, locks) = build_states();
        let sink = InMemoryAuditSink::new();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks)
            .with_driver(Driver::Agent)
            .with_audit_sink(&sink);
        let ran = Arc::new(AtomicBool::new(false));
        let reg = build_registry(Risk::Reversible, &ran);

        dispatch(&reg, "probe", json!({}), &ctx)
            .await
            .expect("Reversible from Agent runs with audit");
        assert!(ran.load(Ordering::SeqCst));
        assert_eq!(sink.entries()[0].driver, Driver::Agent);
    }

    #[tokio::test]
    async fn test_dispatch_default_sink_persists_to_sqlite() {
        // シンク未指定の既定は SqliteAuditSink（本番と同じ経路）
        let (db, pending, batches, locks) = build_states();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks);
        let ran = Arc::new(AtomicBool::new(false));
        let reg = build_registry(Risk::Reversible, &ran);

        dispatch(&reg, "probe", json!({}), &ctx).await.unwrap();

        let rows = db
            .with_conn(|conn| crate::db::audit_log::list_recent(conn, 10))
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].use_case, "probe");
        assert_eq!(rows[0].risk, "reversible");
    }
}
