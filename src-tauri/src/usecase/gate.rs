use crate::usecase::{Driver, Risk};

/// Risk ゲートの判定結果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateOutcome {
    /// 実行してよい（Reversible/Sensitive は dispatch が監査ログに記録する）。
    Allow,
    /// 実行せず承認キューへ積んで保留する（dispatch が投入を行う）。
    RequireApproval,
}

/// Risk ゲート本体（ADR 0004 Phase 4-4）。driver × Risk の認可マトリクス。
///
/// | Risk \ Driver | Ui | Mcp | Agent |
/// |---|---|---|---|
/// | Read | Allow | Allow | Allow |
/// | Reversible | Allow+監査 | Allow+監査 | Allow+監査 |
/// | Sensitive | Allow+監査（人間クリック=承認済み） | 承認キュー | 承認キュー |
///
/// 監査・キュー投入の実体は dispatch 側（use case 名と input が要るため）。
pub fn check(risk: Risk, driver: Driver) -> GateOutcome {
    match (risk, driver) {
        (Risk::Read | Risk::Reversible, _) => GateOutcome::Allow,
        // UI の Sensitive は人間の明示操作そのものが承認
        (Risk::Sensitive, Driver::Ui) => GateOutcome::Allow,
        // LLM 起点の Sensitive は人間の承認まで保留（Phase 5-2 の承認 UI で消費）
        (Risk::Sensitive, Driver::Mcp | Driver::Agent) => GateOutcome::RequireApproval,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usecase::{Driver, Risk};

    #[test]
    fn test_read_and_reversible_pass_for_all_drivers() {
        for driver in [Driver::Ui, Driver::Mcp, Driver::Agent] {
            assert_eq!(
                check(Risk::Read, driver),
                GateOutcome::Allow,
                "Read は {driver:?} で通過する"
            );
            assert_eq!(
                check(Risk::Reversible, driver),
                GateOutcome::Allow,
                "Reversible は {driver:?} で通過する（監査は dispatch 側）"
            );
        }
    }

    #[test]
    fn test_sensitive_from_ui_is_allowed() {
        // 人間の UI 操作は承認済み扱い（ADR 0004 D4）
        assert_eq!(check(Risk::Sensitive, Driver::Ui), GateOutcome::Allow);
    }

    #[test]
    fn test_sensitive_from_llm_drivers_requires_approval() {
        assert_eq!(
            check(Risk::Sensitive, Driver::Mcp),
            GateOutcome::RequireApproval
        );
        assert_eq!(
            check(Risk::Sensitive, Driver::Agent),
            GateOutcome::RequireApproval
        );
    }
}
