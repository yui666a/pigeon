use std::io::Write;

use serde_json::Value;

use crate::usecase::ProgressSink;

/// 進捗イベントを 1 行の表示文字列にする純関数。
///
/// 出力先を持たないので単体でテストできる。`{done}/{total}` 形式を優先し、
/// 想定外の payload では event 名だけを返す（進捗表示のために本処理を
/// 落とさない）。
pub fn format_progress(event: &str, payload: &Value) -> String {
    for key in ["done", "current"] {
        if let (Some(done), Some(total)) = (payload.get(key), payload.get("total")) {
            return format!("{event}: {done}/{total}");
        }
    }
    event.to_string()
}

/// 進捗を stderr に出す ProgressSink。stdout は結果専用に保つため。
pub struct StderrProgressSink;

impl ProgressSink for StderrProgressSink {
    fn emit(&self, event: &str, payload: &Value) {
        let line = format_progress(event, payload);
        // 進捗はベストエフォート。書き込み失敗で本処理を止めない。
        let mut err = std::io::stderr();
        let _ = writeln!(err, "{line}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_done_and_total_are_rendered_as_fraction() {
        let payload = serde_json::json!({"done": 3, "total": 10});
        assert_eq!(
            format_progress("sync-progress", &payload),
            "sync-progress: 3/10"
        );
    }

    #[test]
    fn test_current_is_used_when_done_is_absent() {
        let payload = serde_json::json!({"current": 2, "total": 5});
        assert_eq!(format_progress("classify", &payload), "classify: 2/5");
    }

    #[test]
    fn test_unexpected_payload_falls_back_to_event_name() {
        let payload = serde_json::json!({"unexpected": true});
        assert_eq!(format_progress("sync-progress", &payload), "sync-progress");
    }

    #[test]
    fn test_total_alone_is_not_a_fraction() {
        // 分子が無ければ分数にしない（"sync-progress: /10" を出さない）
        let payload = serde_json::json!({"total": 10});
        assert_eq!(format_progress("sync-progress", &payload), "sync-progress");
    }

    #[test]
    fn test_real_sync_progress_payload_is_rendered() {
        // usecase::cases::sync が実際に送る形（account_id / done / total）。
        // 余分なキーがあっても分数表示になることを固定する。
        let payload = serde_json::json!({"account_id": "a1", "done": 7, "total": 20});
        assert_eq!(
            format_progress("sync-progress", &payload),
            "sync-progress: 7/20"
        );
    }

    #[test]
    fn test_emit_does_not_panic_on_unexpected_payload() {
        StderrProgressSink.emit("sync-progress", &serde_json::json!({"unexpected": true}));
    }
}
