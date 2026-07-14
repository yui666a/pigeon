use crate::error::AppError;
use crate::usecase::{Driver, Risk};

/// Risk ゲート。実行してよいか（誰が）の認可判定。
/// 4-2 では Read のみ通過。Reversible/Sensitive のゲート本体（承認キュー投入・
/// driver 分岐）は Phase 4-4。read 系しか載らないため実害はない。
pub fn check(risk: Risk, _driver: Driver) -> Result<(), AppError> {
    match risk {
        Risk::Read => Ok(()),
        Risk::Reversible | Risk::Sensitive => Err(AppError::Validation(format!(
            "risk gate not yet implemented for {risk:?} (Phase 4-4)"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;
    use crate::usecase::{Driver, Risk};

    #[test]
    fn test_read_passes_for_all_drivers() {
        for driver in [Driver::Ui, Driver::Mcp, Driver::Agent] {
            assert!(
                check(Risk::Read, driver).is_ok(),
                "Read は {driver:?} で通過する"
            );
        }
    }

    #[test]
    fn test_reversible_is_rejected() {
        let err = check(Risk::Reversible, Driver::Ui).expect_err("Reversible は 4-2 では拒否");
        assert!(matches!(err, AppError::Validation(_)));
    }

    #[test]
    fn test_sensitive_is_rejected() {
        let err = check(Risk::Sensitive, Driver::Ui).expect_err("Sensitive は 4-2 では拒否");
        assert!(matches!(err, AppError::Validation(_)));
    }
}
