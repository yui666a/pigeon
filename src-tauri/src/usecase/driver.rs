/// 操作の呼び出し元。ゲートの判定材料になる（分岐の中身は Phase 5）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Driver {
    /// 人間の UI 操作（承認済み扱い）。commands はすべてこれ。
    Ui,
    /// 外部 LLM（MCP 経由）。Phase 5-1。
    Mcp,
    /// 常駐エージェント。Phase 5-3。
    Agent,
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
    fn test_driver_is_copy() {
        let d = Driver::Ui;
        let a = d;
        let b = d;
        assert_eq!(a, b);
    }
}
