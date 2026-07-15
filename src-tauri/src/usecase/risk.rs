/// 操作の危険度分類。UseCase が宣言し、dispatch のゲートが一元的に判定する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Risk {
    /// 自由に実行してよい（検索・一覧）。
    Read,
    /// 自動実行 + 監査（フラグ・未読戻し・案件移動）。ゲート実装は Phase 4-4。
    Reversible,
    /// 人間の承認必須（送信・サーバー削除）。ゲート実装は Phase 4-4。
    Sensitive,
}

impl Risk {
    /// 監査ログ等の永続表現（小文字固定）。
    pub fn as_str(self) -> &'static str {
        match self {
            Risk::Read => "read",
            Risk::Reversible => "reversible",
            Risk::Sensitive => "sensitive",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_risk_variants_are_distinct() {
        assert_ne!(Risk::Read, Risk::Reversible);
        assert_ne!(Risk::Reversible, Risk::Sensitive);
        assert_ne!(Risk::Read, Risk::Sensitive);
    }

    #[test]
    fn test_risk_is_copy() {
        let r = Risk::Read;
        let a = r;
        let b = r; // Copy なので r は move されない
        assert_eq!(a, b);
    }
}
