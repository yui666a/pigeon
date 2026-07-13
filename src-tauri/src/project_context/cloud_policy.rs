use crate::models::directory::CloudRule;

/// クラウド送信可否の判定（スペック§5 不変条件）:
/// - マッチするルールが無ければ常に false（危険側に倒れない）
/// - 最長 relative_path のルールが勝つ。同長なら file スコープが勝つ
/// - directory スコープは prefix マッチ（'' は全体）、file スコープは完全一致
///
/// # 前提条件
/// - `relative_path` は scanner が生成する正規化済み相対パス（`..` セグメントを
///   含まない）を前提とする。`..` セグメントを含むパスが渡された場合は、
///   不変条件「曖昧なら false」に従い無条件で `false` を返す（防御的チェック）。
/// - `rules` には単一ディレクトリ（`list_rules` の出力）のルールのみを渡すこと。
///   複数ディレクトリのルールを結合して渡してはならない（ディレクトリを跨いだ
///   relative_path の意味的な衝突・誤マッチを避けるため）。
pub fn is_cloud_allowed(rules: &[CloudRule], relative_path: &str) -> bool {
    if contains_dotdot_segment(relative_path) {
        return false;
    }

    let mut best: Option<&CloudRule> = None;
    for rule in rules {
        let matches = match rule.scope.as_str() {
            "file" => rule.relative_path == relative_path,
            "directory" => {
                rule.relative_path.is_empty()
                    || relative_path == rule.relative_path
                    || relative_path.starts_with(&format!("{}/", rule.relative_path))
            }
            _ => false,
        };
        if !matches {
            continue;
        }
        best = match best {
            None => Some(rule),
            Some(current) => {
                let longer = rule.relative_path.len() > current.relative_path.len();
                let same_len_file_wins = rule.relative_path.len() == current.relative_path.len()
                    && rule.scope == "file"
                    && current.scope != "file";
                if longer || same_len_file_wins {
                    Some(rule)
                } else {
                    Some(current)
                }
            }
        };
    }
    best.map(|r| r.allow).unwrap_or(false)
}

/// パスが `..` セグメントを含むかどうかを判定する（`"."`区切りではなく `/` 区切りの
/// パスセグメント単位で判定する。`"..foo"` のような紛らわしい名前は対象外）。
fn contains_dotdot_segment(path: &str) -> bool {
    path.split('/').any(|segment| segment == "..")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::directory::CloudRule;

    fn rule(scope: &str, path: &str, allow: bool) -> CloudRule {
        CloudRule {
            id: format!("r-{}-{}", scope, path),
            directory_id: "d1".to_string(),
            scope: scope.to_string(),
            relative_path: path.to_string(),
            allow,
        }
    }

    #[test]
    fn test_no_rules_means_denied() {
        assert!(!is_cloud_allowed(&[], "図面/平面図.pdf"));
    }

    #[test]
    fn test_directory_allow_covers_children() {
        let rules = vec![rule("directory", "図面", true)];
        assert!(is_cloud_allowed(&rules, "図面/平面図.pdf"));
        assert!(is_cloud_allowed(&rules, "図面/sub/詳細.pdf"));
        assert!(!is_cloud_allowed(&rules, "契約/見積.pdf"), "許可外はfalse");
        assert!(
            !is_cloud_allowed(&rules, "図面外.txt"),
            "前方一致の誤爆をしない"
        );
    }

    #[test]
    fn test_root_directory_rule_covers_all() {
        let rules = vec![rule("directory", "", true)];
        assert!(is_cloud_allowed(&rules, "anything.txt"));
        assert!(is_cloud_allowed(&rules, "a/b/c.txt"));
    }

    #[test]
    fn test_explicit_file_deny_beats_parent_allow() {
        let rules = vec![
            rule("directory", "", true),
            rule("file", "予算メモ.md", false),
        ];
        assert!(is_cloud_allowed(&rules, "他.txt"));
        assert!(
            !is_cloud_allowed(&rules, "予算メモ.md"),
            "明示除外が親許可に勝つ"
        );
    }

    #[test]
    fn test_longest_match_wins() {
        let rules = vec![
            rule("directory", "図面", true),
            rule("directory", "図面/社外秘", false),
        ];
        assert!(is_cloud_allowed(&rules, "図面/平面図.pdf"));
        assert!(!is_cloud_allowed(&rules, "図面/社外秘/原価.txt"));
    }

    #[test]
    fn test_file_scope_requires_exact_match() {
        let rules = vec![rule("file", "香盤表.md", true)];
        assert!(is_cloud_allowed(&rules, "香盤表.md"));
        assert!(!is_cloud_allowed(&rules, "香盤表.md.bak"));
        assert!(!is_cloud_allowed(&rules, "sub/香盤表.md"));
    }

    #[test]
    fn test_file_scope_beats_directory_scope_at_same_length() {
        let rules = vec![
            rule("directory", "a/b.txt", true), // 不正気味なルールでも
            rule("file", "a/b.txt", false),     // fileスコープが勝つ
        ];
        assert!(!is_cloud_allowed(&rules, "a/b.txt"));
    }

    #[test]
    fn test_file_scope_beats_directory_scope_at_same_length_reversed_order() {
        // 挿入順（file→directory）を逆にしても結果が変わらないことを固定する
        let rules = vec![
            rule("file", "a/b.txt", false),     // fileスコープが勝つ
            rule("directory", "a/b.txt", true), // 不正気味なルールでも
        ];
        assert!(!is_cloud_allowed(&rules, "a/b.txt"));
    }

    #[test]
    fn test_dotdot_segment_is_always_denied() {
        let rules = vec![rule("directory", "図面", true)];
        assert!(
            !is_cloud_allowed(&rules, "図面/../契約/x.pdf"),
            "..セグメントを含むパスはallowルールに前方一致させず拒否する"
        );
        assert!(!is_cloud_allowed(&rules, ".."), "..単体も拒否する");
        assert!(!is_cloud_allowed(&rules, "../図面/x.pdf"));
        assert!(!is_cloud_allowed(&rules, "図面/.."));
    }
}
