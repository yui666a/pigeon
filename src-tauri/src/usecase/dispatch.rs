use serde_json::Value;

use crate::context::Ctx;
use crate::error::AppError;
use crate::usecase::{gate, AuditEntry, Registry, Risk};

/// 単一の chokepoint。3 driver すべてがこの 1 関数を通る（特権的な裏口なし）。
/// lookup → risk 判定 → ゲート → 監査 → 実行 のパイプライン。
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
    gate::check(risk, ctx.driver())?;

    // 4-2 では Read のみ載るため実質未発火。記録の実体（SQLite）は 4-4。
    if risk != Risk::Read {
        ctx.audit()
            .record(AuditEntry::new(name, risk, ctx.driver()));
    }

    uc.run_json(input, ctx).await
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use serde::{Deserialize, Serialize};
    use serde_json::json;

    use super::*;
    use crate::classifier::service::{ClassifyBatches, PendingClassifications};
    use crate::context::Ctx;
    use crate::error::AppError;
    use crate::state::{DbState, SyncLocks};
    use crate::test_helpers::setup_db;
    use crate::usecase::{Registry, Risk, UseCase};

    #[derive(Deserialize)]
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

    // ゲートに弾かれることを見るためのダミー Reversible use case
    #[derive(Deserialize)]
    struct NoInput {}
    #[derive(Serialize)]
    struct NoOutput {}
    struct DangerUseCase;
    #[async_trait::async_trait]
    impl UseCase for DangerUseCase {
        type Input = NoInput;
        type Output = NoOutput;
        fn name(&self) -> &'static str {
            "danger"
        }
        fn risk(&self, _input: &Self::Input, _ctx: &Ctx) -> Result<Risk, AppError> {
            Ok(Risk::Reversible)
        }
        async fn run(&self, _input: Self::Input, _ctx: &Ctx) -> Result<Self::Output, AppError> {
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

    fn build_registry() -> Registry {
        let mut reg = Registry::new();
        reg.register(EchoUseCase);
        reg.register(DangerUseCase);
        reg
    }

    #[tokio::test]
    async fn test_dispatch_read_usecase_succeeds() {
        let (db, pending, batches, locks) = build_states();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks);
        let reg = build_registry();

        let out = dispatch(&reg, "echo", json!({ "text": "hi" }), &ctx)
            .await
            .expect("read use case should dispatch");
        assert_eq!(out, json!({ "echoed": "hi" }));
    }

    #[tokio::test]
    async fn test_dispatch_unknown_name_errors() {
        let (db, pending, batches, locks) = build_states();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks);
        let reg = build_registry();

        let err = dispatch(&reg, "nope", json!({}), &ctx)
            .await
            .expect_err("unknown name should error");
        assert!(matches!(err, AppError::Validation(_)));
    }

    #[tokio::test]
    async fn test_dispatch_reversible_is_gated() {
        let (db, pending, batches, locks) = build_states();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks);
        let reg = build_registry();

        let err = dispatch(&reg, "danger", json!({}), &ctx)
            .await
            .expect_err("Reversible should be gated in 4-2");
        assert!(matches!(err, AppError::Validation(_)));
    }
}
