use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON-RPC 2.0 のリクエスト。MCP は 1 行 1 メッセージで流れてくる。
#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    #[allow(dead_code)]
    pub jsonrpc: String,
    /// 通知（notification）には id が無い
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: &'static str,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
}

impl JsonRpcResponse {
    pub fn success(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn failure(id: Value, code: i32, message: String) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError { code, message }),
        }
    }
}

/// JSON-RPC 2.0 の標準エラーコード
pub const METHOD_NOT_FOUND: i32 = -32601;
pub const INTERNAL_ERROR: i32 = -32603;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_parses_without_id() {
        let raw = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
        let req: JsonRpcRequest = serde_json::from_str(raw).expect("parse");
        assert!(req.id.is_none());
        assert_eq!(req.method, "notifications/initialized");
    }

    #[test]
    fn test_success_response_omits_error_field() {
        let res = JsonRpcResponse::success(serde_json::json!(1), serde_json::json!({"ok": true}));
        let s = serde_json::to_string(&res).expect("serialize");
        assert!(!s.contains("error"), "{s}");
        assert!(s.contains(r#""id":1"#), "{s}");
    }

    #[test]
    fn test_failure_response_omits_result_field() {
        let res = JsonRpcResponse::failure(serde_json::json!(2), METHOD_NOT_FOUND, "nope".into());
        let s = serde_json::to_string(&res).expect("serialize");
        assert!(!s.contains("result"), "{s}");
        assert!(s.contains("-32601"), "{s}");
    }
}
