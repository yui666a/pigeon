use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

use crate::context::Ctx;
use crate::error::AppError;
use crate::usecase::Risk;

/// 実装者が書く型安全な use case。関連型で Input/Output を型付けする。
/// `run` は async（Sensitive 抽出で IMAP/SMTP を伴う use case が載るため。
/// ADR 0004 の 4-5 前倒し）。同期処理はそのまま await なしで書けばよい。
/// Send + Sync は Tauri の managed State（レジストリ経由の dyn 共有）に載せるための境界。
#[async_trait::async_trait]
pub trait UseCase: Send + Sync {
    type Input: DeserializeOwned + Send + schemars::JsonSchema;
    type Output: Serialize;

    fn name(&self) -> &'static str;

    /// 実効 Risk。input と ctx を参照できる（archive のプラン依存 Risk は
    /// mail_policy の実効プランを DB から引いて決まる。ADR 0004 D3）。
    /// 固定 Risk の use case は input / ctx を無視して定数を返す。
    fn risk(&self, input: &Self::Input, ctx: &Ctx) -> Result<Risk, AppError>;

    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError>;
}

/// dyn 化のための消去層。`serde_json::Value` 境界で叩ける。
/// 実装は下のブランケットで `UseCase` から自動導出される（手書き不要）。
#[async_trait::async_trait]
pub trait ErasedUseCase: Send + Sync {
    fn name(&self) -> &str;
    /// Input 型から導出した JSON Schema。MCP の tools/list と CLI の引数検証が共用する。
    fn input_schema(&self) -> Value;
    fn risk_json(&self, input: &Value, ctx: &Ctx) -> Result<Risk, AppError>;
    async fn run_json(&self, input: Value, ctx: &Ctx) -> Result<Value, AppError>;
}

#[async_trait::async_trait]
impl<T: UseCase> ErasedUseCase for T {
    fn name(&self) -> &str {
        UseCase::name(self)
    }

    /// schema のシリアライズは実質失敗しないが、ここで Result を返すと
    /// 呼び出し側（tools/list や --list）が一様に煩雑になるため、
    /// 失敗時は最小の object schema にフォールバックする。
    fn input_schema(&self) -> Value {
        let schema = schemars::schema_for!(T::Input);
        serde_json::to_value(schema).unwrap_or_else(|_| serde_json::json!({"type": "object"}))
    }

    fn risk_json(&self, input: &Value, ctx: &Ctx) -> Result<Risk, AppError> {
        let typed: T::Input = serde_json::from_value(input.clone()).map_err(|e| {
            AppError::Validation(format!("invalid input for {}: {e}", UseCase::name(self)))
        })?;
        self.risk(&typed, ctx)
    }

    async fn run_json(&self, input: Value, ctx: &Ctx) -> Result<Value, AppError> {
        let typed: T::Input = serde_json::from_value(input).map_err(|e| {
            AppError::Validation(format!("invalid input for {}: {e}", UseCase::name(self)))
        })?;
        let output = self.run(typed, ctx).await?;
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

    /// run 内で実際に await する use case（async 化の回帰テスト用）。
    struct AsyncEchoUseCase;

    #[async_trait::async_trait]
    impl UseCase for AsyncEchoUseCase {
        type Input = EchoInput;
        type Output = EchoOutput;

        fn name(&self) -> &'static str {
            "async_echo"
        }

        fn risk(&self, _input: &Self::Input, _ctx: &Ctx) -> Result<Risk, AppError> {
            Ok(Risk::Read)
        }

        async fn run(&self, input: Self::Input, _ctx: &Ctx) -> Result<Self::Output, AppError> {
            tokio::task::yield_now().await;
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

    #[tokio::test]
    async fn test_erased_run_json_roundtrips() {
        let (db, pending, batches, locks) = build_states();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks);
        let uc = EchoUseCase;

        let out = uc
            .run_json(json!({ "text": "hi" }), &ctx)
            .await
            .expect("run_json should succeed");
        assert_eq!(out, json!({ "echoed": "hi" }));
    }

    #[tokio::test]
    async fn test_erased_run_json_supports_awaiting_usecases() {
        // run 内で await する use case も消去層経由で動く（async 化の主目的）
        let (db, pending, batches, locks) = build_states();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks);
        let uc = AsyncEchoUseCase;

        let out = uc
            .run_json(json!({ "text": "later" }), &ctx)
            .await
            .expect("async run_json should succeed");
        assert_eq!(out, json!({ "echoed": "later" }));
    }

    #[test]
    fn test_erased_name_delegates() {
        let uc = EchoUseCase;
        assert_eq!(ErasedUseCase::name(&uc), "echo");
    }

    #[test]
    fn test_erased_risk_json_reads_input() {
        let (db, pending, batches, locks) = build_states();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks);
        let uc = EchoUseCase;
        let risk = uc
            .risk_json(&json!({ "text": "hi" }), &ctx)
            .expect("risk_json should parse input");
        assert_eq!(risk, Risk::Read);
    }

    #[tokio::test]
    async fn test_erased_run_json_rejects_bad_input() {
        let (db, pending, batches, locks) = build_states();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks);
        let uc = EchoUseCase;

        // text フィールドが無い → deserialize 失敗 → AppError::Validation
        let err = uc
            .run_json(json!({ "wrong": "field" }), &ctx)
            .await
            .expect_err("should reject invalid input");
        assert!(matches!(err, AppError::Validation(_)));
    }
}
