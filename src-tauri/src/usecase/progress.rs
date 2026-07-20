use serde_json::Value;

/// 長時間処理の進捗通知。driver ごとに出力先が異なる（GUI: emit / CLI: stderr / MCP: 破棄）。
/// 送出失敗で本処理を止めない（ベストエフォート）。
pub trait ProgressSink: Send + Sync {
    fn emit(&self, event: &str, payload: &Value);
}

/// 進捗を捨てる既定実装。
pub struct NoOpProgressSink;

impl ProgressSink for NoOpProgressSink {
    fn emit(&self, _event: &str, _payload: &Value) {}
}

#[cfg(test)]
pub struct RecordingProgressSink {
    pub events: std::sync::Mutex<Vec<(String, Value)>>,
}

#[cfg(test)]
impl RecordingProgressSink {
    pub fn new() -> Self {
        Self {
            events: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[cfg(test)]
impl ProgressSink for RecordingProgressSink {
    fn emit(&self, event: &str, payload: &Value) {
        if let Ok(mut v) = self.events.lock() {
            v.push((event.to_string(), payload.clone()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_noop_sink_does_not_panic() {
        NoOpProgressSink.emit("sync-progress", &serde_json::json!({"done": 1}));
    }

    #[test]
    fn test_recording_sink_captures_events() {
        let sink = RecordingProgressSink::new();
        sink.emit(
            "sync-progress",
            &serde_json::json!({"done": 3, "total": 10}),
        );
        let events = sink.events.lock().expect("lock");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "sync-progress");
        assert_eq!(events[0].1["done"], 3);
    }
}
