use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

use crate::context::Ctx;
use crate::error::AppError;
use crate::usecase::Risk;

/// 実装者が書く型安全な use case。関連型で Input/Output を型付けする。
/// `run` は同期（async 対応は Phase 4-5）。
pub trait UseCase {
    type Input: DeserializeOwned;
    type Output: Serialize;

    fn name(&self) -> &'static str;

    /// 実効 Risk。input を参照できる（archive のプラン依存 Risk 等）。
    /// 多くの use case は input を無視して固定 Risk を返す。
    fn risk(&self, input: &Self::Input) -> Risk;

    fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError>;
}

/// dyn 化のための消去層。`serde_json::Value` 境界で叩ける。
/// 実装は下のブランケットで `UseCase` から自動導出される（手書き不要）。
pub trait ErasedUseCase {
    fn name(&self) -> &str;
    fn risk_json(&self, input: &Value) -> Result<Risk, AppError>;
    fn run_json(&self, input: Value, ctx: &Ctx) -> Result<Value, AppError>;
}

impl<T: UseCase> ErasedUseCase for T {
    fn name(&self) -> &str {
        UseCase::name(self)
    }

    fn risk_json(&self, input: &Value) -> Result<Risk, AppError> {
        let typed: T::Input = serde_json::from_value(input.clone()).map_err(|e| {
            AppError::Validation(format!("invalid input for {}: {e}", UseCase::name(self)))
        })?;
        Ok(self.risk(&typed))
    }

    fn run_json(&self, input: Value, ctx: &Ctx) -> Result<Value, AppError> {
        let typed: T::Input = serde_json::from_value(input).map_err(|e| {
            AppError::Validation(format!("invalid input for {}: {e}", UseCase::name(self)))
        })?;
        let output = self.run(typed, ctx)?;
        serde_json::to_value(output)
            .map_err(|e| AppError::Validation(format!("failed to serialize output: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use serde::{Deserialize, Serialize};
    use serde_json::json;

    use super::*;
    use crate::classifier::service::{ClassifyBatches, PendingClassifications};
    use crate::context::Ctx;
    use crate::state::{DbState, SyncLocks};
    use crate::test_helpers::setup_db;
    use crate::usecase::Risk;

    #[derive(Deserialize)]
    struct EchoInput {
        text: String,
    }

    #[derive(Serialize)]
    struct EchoOutput {
        echoed: String,
    }

    struct EchoUseCase;

    impl UseCase for EchoUseCase {
        type Input = EchoInput;
        type Output = EchoOutput;

        fn name(&self) -> &'static str {
            "echo"
        }

        fn risk(&self, _input: &Self::Input) -> Risk {
            Risk::Read
        }

        fn run(&self, input: Self::Input, _ctx: &Ctx) -> Result<Self::Output, AppError> {
            Ok(EchoOutput { echoed: input.text })
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

    #[test]
    fn test_erased_run_json_roundtrips() {
        let (db, pending, batches, locks) = build_states();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks);
        let uc = EchoUseCase;

        let out = uc
            .run_json(json!({ "text": "hi" }), &ctx)
            .expect("run_json should succeed");
        assert_eq!(out, json!({ "echoed": "hi" }));
    }

    #[test]
    fn test_erased_name_delegates() {
        let uc = EchoUseCase;
        assert_eq!(ErasedUseCase::name(&uc), "echo");
    }

    #[test]
    fn test_erased_risk_json_reads_input() {
        let uc = EchoUseCase;
        let risk = uc
            .risk_json(&json!({ "text": "hi" }))
            .expect("risk_json should parse input");
        assert_eq!(risk, Risk::Read);
    }

    #[test]
    fn test_erased_run_json_rejects_bad_input() {
        let (db, pending, batches, locks) = build_states();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks);
        let uc = EchoUseCase;

        // text フィールドが無い → deserialize 失敗 → AppError::Validation
        let err = uc
            .run_json(json!({ "wrong": "field" }), &ctx)
            .expect_err("should reject invalid input");
        assert!(matches!(err, AppError::Validation(_)));
    }
}
