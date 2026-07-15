//! 送信の Sensitive use case（Phase 4-3）。本体は send_commands::send_mail_service。

use crate::commands::send_commands::{send_mail_service, SendMailRequest};
use crate::context::Ctx;
use crate::error::AppError;
use crate::usecase::{Registry, Risk, UseCase};

/// メール送信。取り消し不能な外向き操作の代表であり、常に Sensitive。
pub struct SendMailUseCase;

#[async_trait::async_trait]
impl UseCase for SendMailUseCase {
    type Input = SendMailRequest;
    type Output = ();

    fn name(&self) -> &'static str {
        "send_mail"
    }

    fn risk(&self, _input: &Self::Input, _ctx: &Ctx) -> Result<Risk, AppError> {
        Ok(Risk::Sensitive)
    }

    async fn run(&self, input: Self::Input, ctx: &Ctx) -> Result<Self::Output, AppError> {
        send_mail_service(
            ctx.db(),
            ctx.secure_store()?,
            ctx.approved_attachments()?,
            input,
        )
        .await
    }
}

pub fn register_send_cases(registry: &mut Registry) {
    registry.register(SendMailUseCase);
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

    fn make_request() -> SendMailRequest {
        SendMailRequest {
            account_id: "acc1".into(),
            to: vec!["to@example.com".into()],
            cc: vec![],
            bcc: vec![],
            subject: "件名".into(),
            body_text: "本文".into(),
            body_html: None,
            reply_to_mail_id: None,
            attachments: vec![],
        }
    }

    #[test]
    fn test_send_mail_is_always_sensitive() {
        let (db, pending, batches, locks) = build_states();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks);
        assert_eq!(
            SendMailUseCase.risk(&make_request(), &ctx).unwrap(),
            Risk::Sensitive
        );
    }

    #[tokio::test]
    async fn test_send_mail_requires_approved_attachments_in_ctx() {
        // テスト用 Ctx は許可リスト未設定 → SMTP に触れる前に構成エラーで止まる
        let (db, pending, batches, locks) = build_states();
        let ctx = Ctx::new_for_test(&db, &pending, &batches, &locks);

        let err = SendMailUseCase
            .run(make_request(), &ctx)
            .await
            .expect_err("ctx without approved attachments must be rejected");
        assert!(matches!(err, AppError::Validation(_)));
    }
}
