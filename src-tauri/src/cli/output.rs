use serde_json::Value;

/// dispatch の戻り値を表示用の文字列にする。
/// as_json なら整形済み JSON、そうでなければ人間向けの要約。
pub fn render(value: &Value, as_json: bool) -> String {
    if as_json {
        // 整形に失敗しても表示は止めない（Value の Display は常に成功する）。
        return serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string());
    }
    render_human(value)
}

fn render_human(value: &Value) -> String {
    match value {
        Value::Null => "(no output)".to_string(),
        Value::Array(items) if items.is_empty() => "(empty)".to_string(),
        Value::Array(items) => items.iter().map(render_line).collect::<Vec<_>>().join("\n"),
        other => render_line(other),
    }
}

/// 1 要素を 1 行にする。id / name / subject など代表的なキーを優先して拾う。
fn render_line(value: &Value) -> String {
    let Value::Object(map) = value else {
        return match value {
            // 文字列は引用符なしで出す（`call` の結果をそのままパイプできるように）。
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
    };
    for key in ["subject", "name", "title", "id"] {
        if let Some(Value::String(s)) = map.get(key) {
            return s.clone();
        }
    }
    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_mode_is_pretty_printed() {
        let v = serde_json::json!({"a": 1});
        let out = render(&v, true);
        assert!(out.contains('\n'), "整形されている: {out}");
    }

    #[test]
    fn test_empty_array_is_reported() {
        assert_eq!(render(&serde_json::json!([]), false), "(empty)");
    }

    #[test]
    fn test_null_is_reported() {
        assert_eq!(render(&Value::Null, false), "(no output)");
    }

    #[test]
    fn test_array_of_objects_uses_representative_key() {
        let v = serde_json::json!([
            {"id": "1", "subject": "hello"},
            {"id": "2", "name": "world"}
        ]);
        assert_eq!(render(&v, false), "hello\nworld");
    }

    #[test]
    fn test_scalar_number_is_rendered_as_is() {
        // sync_account の戻り値は取得件数（u32）。JSON の数値がそのまま出る。
        assert_eq!(render(&serde_json::json!(12), false), "12");
    }

    #[test]
    fn test_scalar_string_has_no_quotes() {
        assert_eq!(render(&serde_json::json!("done"), false), "done");
    }

    #[test]
    fn test_object_without_representative_key_falls_back_to_json() {
        // get_unread_counts のような集計オブジェクトは要約キーを持たない。
        let v = serde_json::json!({"unclassified": 3});
        assert_eq!(render(&v, false), "{\"unclassified\":3}");
    }

    #[test]
    fn test_json_mode_preserves_empty_array() {
        // --json は「見やすさ」ではなく機械可読性が目的なので (empty) に潰さない。
        assert_eq!(render(&serde_json::json!([]), true), "[]");
    }
}
