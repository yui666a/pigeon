use crate::error::AppError;
use crate::models::classifier::{ClassifyResult, ProjectSuggestion};

/// 本文中の最初の '{' から最後の '}' までを取り出す。
pub fn extract_json(content: &str) -> Option<&str> {
    let start = content.find('{')?;
    let end = content.rfind('}')?;
    if start <= end {
        Some(&content[start..=end])
    } else {
        None
    }
}

/// LLM 応答テキストから ClassifyResult をパースする。
pub fn parse_classify_result(content: &str) -> Result<ClassifyResult, AppError> {
    let json_str = extract_json(content).ok_or_else(|| {
        AppError::InvalidLlmResponse(format!("No JSON object found in response: {}", content))
    })?;
    serde_json::from_str::<ClassifyResult>(json_str).map_err(|e| {
        AppError::InvalidLlmResponse(format!(
            "Failed to parse ClassifyResult from '{}': {}",
            json_str, e
        ))
    })
}

/// LLM 応答から ProjectSuggestion をパースする。
/// パース不能・フィールド欠落でも Err にせず、埋められた分だけ返す
/// （名前が空ならフォーム側でユーザーが手入力する前提）。
pub fn parse_project_suggestion(content: &str) -> ProjectSuggestion {
    // serde の Deserialize で欠落フィールドを空文字に落とすため、
    // Option で受けてから unwrap_or_default する
    #[derive(serde::Deserialize)]
    struct Raw {
        name: Option<String>,
        description: Option<String>,
    }
    let empty = ProjectSuggestion {
        name: String::new(),
        description: String::new(),
    };
    let Some(json_str) = extract_json(content) else {
        return empty;
    };
    match serde_json::from_str::<Raw>(json_str) {
        Ok(raw) => ProjectSuggestion {
            name: raw.name.unwrap_or_default(),
            description: raw.description.unwrap_or_default(),
        },
        Err(_) => empty,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::classifier::ClassifyAction;

    #[test]
    fn test_extract_json() {
        let input =
            r#"{"action": "assign", "project_id": "p1", "confidence": 0.9, "reason": "test"}"#;
        assert_eq!(extract_json(input).unwrap(), input);
    }

    #[test]
    fn test_extract_json_with_surrounding_text() {
        let input = r#"Sure: {"action": "unclassified", "confidence": 0.2, "reason": "x"} done"#;
        let out = extract_json(input).unwrap();
        assert!(out.starts_with('{') && out.ends_with('}'));
    }

    #[test]
    fn test_extract_json_no_json() {
        assert!(extract_json("no json here").is_none());
    }

    #[test]
    fn test_extract_json_empty_string() {
        assert!(extract_json("").is_none());
    }

    #[test]
    fn test_extract_json_only_open_brace() {
        assert!(extract_json("{").is_none());
    }

    #[test]
    fn test_extract_json_only_close_brace() {
        assert!(extract_json("}").is_none());
    }

    #[test]
    fn test_extract_json_nested_braces() {
        let input = r#"{"outer": {"inner": "value"}}"#;
        assert_eq!(extract_json(input).unwrap(), input);
    }

    #[test]
    fn test_parse_assign() {
        let content =
            r#"{"action": "assign", "project_id": "proj-123", "confidence": 0.85, "reason": "r"}"#;
        let result = parse_classify_result(content).unwrap();
        assert!(matches!(result.action, ClassifyAction::Assign { .. }));
        if let ClassifyAction::Assign { project_id } = result.action {
            assert_eq!(project_id, "proj-123");
        }
        assert!((result.confidence - 0.85).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parse_create() {
        let content = r#"{"action": "create", "project_name": "新規", "description": "d", "confidence": 0.75, "reason": "r"}"#;
        let result = parse_classify_result(content).unwrap();
        assert!(matches!(result.action, ClassifyAction::Create { .. }));
    }

    #[test]
    fn test_parse_unclassified() {
        let content = r#"{"action": "unclassified", "confidence": 0.2, "reason": "曖昧"}"#;
        let result = parse_classify_result(content).unwrap();
        assert!(matches!(result.action, ClassifyAction::Unclassified));
    }

    #[test]
    fn test_parse_with_surrounding_text() {
        let content = "結果:\n{\"action\": \"assign\", \"project_id\": \"p\", \"confidence\": 0.9, \"reason\": \"r\"}\nおわり";
        let result = parse_classify_result(content).unwrap();
        assert!(matches!(result.action, ClassifyAction::Assign { .. }));
    }

    #[test]
    fn test_parse_invalid() {
        assert!(parse_classify_result("plain text").is_err());
    }

    #[test]
    fn test_parse_missing_confidence() {
        assert!(parse_classify_result(r#"{"action": "unclassified", "reason": "t"}"#).is_err());
    }

    #[test]
    fn test_parse_missing_reason() {
        assert!(parse_classify_result(r#"{"action": "unclassified", "confidence": 0.5}"#).is_err());
    }

    #[test]
    fn test_parse_unknown_action() {
        assert!(
            parse_classify_result(r#"{"action": "delete", "confidence": 0.5, "reason": "t"}"#)
                .is_err()
        );
    }

    #[test]
    fn test_parse_assign_missing_project_id() {
        assert!(
            parse_classify_result(r#"{"action": "assign", "confidence": 0.9, "reason": "t"}"#)
                .is_err()
        );
    }

    #[test]
    fn test_parse_create_missing_fields() {
        assert!(
            parse_classify_result(r#"{"action": "create", "confidence": 0.7, "reason": "t"}"#)
                .is_err()
        );
    }

    #[test]
    fn test_parse_confidence_boundaries() {
        let r0 = parse_classify_result(
            r#"{"action": "unclassified", "confidence": 0.0, "reason": "t"}"#,
        )
        .unwrap();
        assert!((r0.confidence - 0.0).abs() < f64::EPSILON);
        let r1 = parse_classify_result(
            r#"{"action": "unclassified", "confidence": 1.0, "reason": "t"}"#,
        )
        .unwrap();
        assert!((r1.confidence - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_parse_project_suggestion_valid() {
        let content = r#"{"name": "在庫管理", "description": "在庫MTGとレポート"}"#;
        let s = parse_project_suggestion(content);
        assert_eq!(s.name, "在庫管理");
        assert_eq!(s.description, "在庫MTGとレポート");
    }

    #[test]
    fn test_parse_project_suggestion_with_surrounding_text() {
        let content = "はい: {\"name\": \"A\", \"description\": \"B\"} 以上";
        let s = parse_project_suggestion(content);
        assert_eq!(s.name, "A");
        assert_eq!(s.description, "B");
    }

    #[test]
    fn test_parse_project_suggestion_invalid_falls_back_to_empty() {
        // パース不能でも Err にせず空フォールバック（フォームで手入力可能）
        let s = parse_project_suggestion("plain text no json");
        assert_eq!(s.name, "");
        assert_eq!(s.description, "");
    }

    #[test]
    fn test_parse_project_suggestion_missing_description() {
        // description 欠落時は空文字で補う（パニックしない）
        let s = parse_project_suggestion(r#"{"name": "只名前"}"#);
        assert_eq!(s.name, "只名前");
        assert_eq!(s.description, "");
    }
}
