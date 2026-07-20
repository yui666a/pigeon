use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::cli::runtime::CliRuntime;
use crate::error::AppError;
use crate::mcp::protocol::{JsonRpcRequest, JsonRpcResponse, INTERNAL_ERROR, METHOD_NOT_FOUND};
use crate::usecase::{cases, dispatch, Driver, Registry, UseCaseInfo};

/// UseCase 情報を MCP の tool 定義に変換する。
pub fn to_tool_definition(info: &UseCaseInfo) -> Value {
    serde_json::json!({
        "name": info.name,
        "description": format!("Pigeon use case: {}", info.name),
        "inputSchema": info.input_schema,
    })
}

/// tools/call の結果を MCP の CallToolResult に変換する。
/// UseCase のエラーは JSON-RPC のエラーではなく `isError: true` の
/// 成功レスポンスで返す（プロトコルエラーと UseCase エラーは別物）。
pub fn to_call_tool_result(outcome: Result<Value, String>) -> Value {
    match outcome {
        Ok(out) => {
            let text = serde_json::to_string_pretty(&out).unwrap_or_else(|_| out.to_string());
            serde_json::json!({ "content": [{ "type": "text", "text": text }] })
        }
        Err(message) => serde_json::json!({
            "content": [{ "type": "text", "text": message }],
            "isError": true
        }),
    }
}

/// MCP サーバーの実行環境。
///
/// ランタイム（DB / SecureStore）は `tools/call` で初めて必要になるため
/// 遅延して開く。`initialize` と `tools/list` は Registry だけで応答でき、
/// これらのために OS キーチェーンへアクセスするのは筋が悪い。
struct ServerState {
    /// tool 一覧の導出専用。ランタイムを開かずに使える。
    registry: Registry,
    runtime: Option<CliRuntime>,
}

impl ServerState {
    fn new() -> Self {
        let mut registry = Registry::new();
        cases::register_all(&mut registry);
        Self {
            registry,
            runtime: None,
        }
    }

    /// 必要になった時点でランタイムを開く。以降は使い回す。
    fn runtime(&mut self) -> Result<&CliRuntime, AppError> {
        if self.runtime.is_none() {
            // MCP 経由であることを監査ログに残すため CliAutomated ではなく Mcp。
            self.runtime = Some(CliRuntime::open(Driver::Mcp)?);
        }
        self.runtime
            .as_ref()
            .ok_or_else(|| AppError::Validation("ランタイムの初期化に失敗しました".to_string()))
    }
}

/// stdio で MCP サーバーを走らせる。
///
/// stdout は JSON-RPC が占有するため、ログ・進捗は一切書かない。
/// 進捗は Ctx に sink を設定しないことで NoOpProgressSink に落ちる。
/// 診断は stderr へ。
pub async fn serve_stdio() -> Result<(), AppError> {
    let mut state = ServerState::new();
    let stdin = tokio::io::stdin();
    let mut lines = BufReader::new(stdin).lines();
    let mut stdout = tokio::io::stdout();

    while let Ok(Some(line)) = lines.next_line().await {
        if line.trim().is_empty() {
            continue;
        }
        let req: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("mcp: 不正なリクエストを無視しました: {e}");
                continue;
            }
        };
        // 通知（id なし）には応答しない
        let Some(id) = req.id.clone() else { continue };

        let response = handle_request(&mut state, &req, id).await;
        let body = match serde_json::to_string(&response) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("mcp: 応答のシリアライズに失敗しました: {e}");
                continue;
            }
        };
        if stdout.write_all(body.as_bytes()).await.is_err()
            || stdout.write_all(b"\n").await.is_err()
            || stdout.flush().await.is_err()
        {
            // クライアントが切断した
            break;
        }
    }
    Ok(())
}

async fn handle_request(
    state: &mut ServerState,
    req: &JsonRpcRequest,
    id: Value,
) -> JsonRpcResponse {
    match req.method.as_str() {
        "initialize" => JsonRpcResponse::success(
            id,
            serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "pigeon", "version": env!("CARGO_PKG_VERSION") }
            }),
        ),
        "tools/list" => {
            let tools: Vec<Value> = state
                .registry
                .describe()
                .iter()
                .map(to_tool_definition)
                .collect();
            JsonRpcResponse::success(id, serde_json::json!({ "tools": tools }))
        }
        "tools/call" => {
            let name = req
                .params
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let args = req
                .params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));

            // ランタイムを開けない（GUI 起動中など）のはプロトコル以前の
            // 環境エラーなので JSON-RPC エラーで返す。
            let runtime = match state.runtime() {
                Ok(r) => r,
                Err(e) => {
                    return JsonRpcResponse::failure(id, INTERNAL_ERROR, e.to_string());
                }
            };
            // progress は設定しない = NoOpProgressSink。stdout を汚さない。
            let ctx = runtime.ctx();
            let outcome = dispatch(runtime.registry(), &name, args, &ctx)
                .await
                .map_err(|e| e.to_string());
            JsonRpcResponse::success(id, to_call_tool_result(outcome))
        }
        other => JsonRpcResponse::failure(
            id,
            METHOD_NOT_FOUND,
            format!("未対応のメソッドです: {other}"),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_definition_has_required_mcp_fields() {
        let info = UseCaseInfo {
            name: "search_mails",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {"query": {"type": "string"}}
            }),
        };
        let tool = to_tool_definition(&info);
        assert_eq!(tool["name"], "search_mails");
        assert!(tool["description"].is_string());
        assert_eq!(tool["inputSchema"]["type"], "object");
    }

    #[test]
    fn test_call_tool_result_success_has_no_is_error() {
        let result = to_call_tool_result(Ok(serde_json::json!({"count": 3})));
        assert_eq!(result["content"][0]["type"], "text");
        assert!(result["content"][0]["text"]
            .as_str()
            .is_some_and(|t| t.contains("count")));
        assert!(result.get("isError").is_none());
    }

    #[test]
    fn test_call_tool_result_error_is_success_with_is_error_flag() {
        // UseCase のエラーは JSON-RPC エラーではなく isError で返す
        let result = to_call_tool_result(Err("approval required".to_string()));
        assert_eq!(result["isError"], true);
        assert_eq!(result["content"][0]["text"], "approval required");
    }

    /// tool 一覧はランタイム（DB / SecureStore）に触らず導出できる。
    #[test]
    fn test_tools_are_derivable_without_runtime() {
        let state = ServerState::new();
        assert!(state.runtime.is_none());
        let tools: Vec<Value> = state
            .registry
            .describe()
            .iter()
            .map(to_tool_definition)
            .collect();
        assert!(!tools.is_empty());
        assert!(tools.iter().any(|t| t["name"] == "search_mails"));
    }
}
