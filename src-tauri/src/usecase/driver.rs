/// 操作の呼び出し元。ゲートの判定材料になる（分岐の中身は Phase 5）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Driver {
    /// 人間の UI 操作（承認済み扱い）。commands はすべてこれ。
    Ui,
    /// 対話端末から起動された CLI。人間の明示操作とみなす
    CliInteractive,
    /// 非対話（パイプ・エージェント経由）で起動された CLI
    CliAutomated,
    /// 外部 LLM（MCP 経由）。Phase 5-1。
    Mcp,
    /// 常駐エージェント。Phase 5-3。
    Agent,
}

impl Driver {
    /// 監査ログ・承認キューの永続表現（小文字固定）。
    pub fn as_str(self) -> &'static str {
        match self {
            Driver::Ui => "ui",
            Driver::CliInteractive => "cli_interactive",
            Driver::CliAutomated => "cli_automated",
            Driver::Mcp => "mcp",
            Driver::Agent => "agent",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_driver_variants_are_distinct() {
        assert_ne!(Driver::Ui, Driver::Mcp);
        assert_ne!(Driver::Mcp, Driver::Agent);
        assert_ne!(Driver::Ui, Driver::Agent);
    }

    #[test]
    fn test_cli_drivers_as_str() {
        assert_eq!(Driver::CliInteractive.as_str(), "cli_interactive");
        assert_eq!(Driver::CliAutomated.as_str(), "cli_automated");
    }

    #[test]
    fn test_driver_is_copy() {
        let d = Driver::Ui;
        let a = d;
        let b = d;
        assert_eq!(a, b);
    }
}
