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

/// 1 要素を 1 行にする。`id` と、subject / email / name / title のうち
/// 最初に見つかった代表キーを併記する。
///
/// id を必ず出すのは、CLI の他コマンドが account_id / project_id を引数に取る
/// ためで、一覧の主目的が「その id を知ること」だから。代表キーだけ出すと
/// 一覧を見ても次のコマンドが打てない。email を name より優先するのは、
/// アカウントは表示名よりメールアドレスの方が本人にとって識別しやすいため。
fn render_line(value: &Value) -> String {
    let Value::Object(map) = value else {
        return match value {
            // 文字列は引用符なしで出す（`call` の結果をそのままパイプできるように）。
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
    };
    let id = match map.get("id") {
        Some(Value::String(s)) => Some(s.as_str()),
        _ => None,
    };
    let label = ["subject", "email", "name", "title"]
        .iter()
        .find_map(|key| match map.get(*key) {
            Some(Value::String(s)) => Some(s.as_str()),
            _ => None,
        });
    match (id, label) {
        (Some(id), Some(label)) => format!("{id}  {label}"),
        (Some(id), None) => id.to_string(),
        (None, Some(label)) => label.to_string(),
        (None, None) => value.to_string(),
    }
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
        assert_eq!(render(&v, false), "1  hello\n2  world");
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
    fn test_id_is_shown_alongside_the_representative_key() {
        // CLI の他コマンドは account_id / project_id を引数に取るので、
        // 一覧の目的は「その id を知ること」。id は必ず併記する。
        let v = serde_json::json!([{"id": "p-1", "name": "ツアー案件"}]);
        assert_eq!(render(&v, false), "p-1  ツアー案件");
    }

    #[test]
    fn test_email_is_shown_when_present() {
        // アカウントは name より email の方が本人にとって識別しやすい。
        let v = serde_json::json!([{"id": "acc-1", "name": "Work", "email": "w@example.com"}]);
        assert!(
            render(&v, false).contains("w@example.com"),
            "email が出る: {}",
            render(&v, false)
        );
    }

    #[test]
    fn test_id_only_object_is_not_duplicated() {
        // id しか無い要素で "acc-1  acc-1" のように重複させない。
        assert_eq!(
            render(&serde_json::json!([{"id": "acc-1"}]), false),
            "acc-1"
        );
    }

    #[test]
    fn test_json_mode_preserves_empty_array() {
        // --json は「見やすさ」ではなく機械可読性が目的なので (empty) に潰さない。
        assert_eq!(render(&serde_json::json!([]), true), "[]");
    }
}
