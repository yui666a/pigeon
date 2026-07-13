//! Anthropic Messages API の共通型とレスポンス処理。
//!
//! 直 API（`claude.rs`）と Vertex 経由（`claude_vertex.rs`）はリクエストボディの
//! 外形（`model` の位置や `anthropic_version` の指定方法）が異なるが、
//! `messages` 配列の要素とレスポンス（content ブロック）は同形なのでここに集約する。

use serde::{Deserialize, Serialize};

use crate::error::AppError;

/// Messages API の `messages` 配列の要素。
#[derive(Debug, Serialize)]
pub(crate) struct MessageParam {
    pub(crate) role: String,
    pub(crate) content: String,
}

/// user ロール 1 件だけの `messages` を組み立てる。
pub(crate) fn user_messages(user_prompt: &str) -> Vec<MessageParam> {
    vec![MessageParam {
        role: "user".to_string(),
        content: user_prompt.to_string(),
    }]
}

/// Messages API のレスポンス（直 API / Vertex rawPredict で同形）。
#[derive(Debug, Deserialize)]
pub(crate) struct MessagesResponse {
    pub(crate) content: Vec<ContentBlock>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ContentBlock {
    #[serde(rename = "type")]
    pub(crate) block_type: String,
    pub(crate) text: Option<String>,
}

/// レスポンス JSON から最初の text ブロックを取り出す。
pub(crate) fn extract_text(resp: &MessagesResponse) -> Result<String, AppError> {
    resp.content
        .iter()
        .find_map(|b| {
            if b.block_type == "text" {
                b.text.clone()
            } else {
                None
            }
        })
        .ok_or_else(|| AppError::InvalidLlmResponse("no text block in response".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_messages_builds_single_user_message() {
        let messages = user_messages("こんにちは");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content, "こんにちは");
    }

    #[test]
    fn test_message_param_serializes_role_and_content() {
        let json = serde_json::to_value(user_messages("usr")).unwrap();
        assert_eq!(json[0]["role"], "user");
        assert_eq!(json[0]["content"], "usr");
    }

    #[test]
    fn test_extract_text_finds_text_block() {
        let resp = MessagesResponse {
            content: vec![ContentBlock {
                block_type: "text".to_string(),
                text: Some("{\"action\":\"unclassified\"}".to_string()),
            }],
        };
        assert_eq!(
            extract_text(&resp).unwrap(),
            "{\"action\":\"unclassified\"}"
        );
    }

    #[test]
    fn test_extract_text_skips_non_text_blocks() {
        let resp = MessagesResponse {
            content: vec![
                ContentBlock {
                    block_type: "tool_use".to_string(),
                    text: None,
                },
                ContentBlock {
                    block_type: "text".to_string(),
                    text: Some("hello".to_string()),
                },
            ],
        };
        assert_eq!(extract_text(&resp).unwrap(), "hello");
    }

    #[test]
    fn test_extract_text_no_text_block_errs() {
        let resp = MessagesResponse {
            content: vec![ContentBlock {
                block_type: "tool_use".to_string(),
                text: None,
            }],
        };
        assert!(extract_text(&resp).is_err());
    }

    #[test]
    fn test_extract_text_empty_content_errs() {
        let resp = MessagesResponse { content: vec![] };
        assert!(extract_text(&resp).is_err());
    }

    #[test]
    fn test_response_deserializes_from_api_json() {
        let json = r#"{"content":[{"type":"text","text":"hello"}]}"#;
        let resp: MessagesResponse = serde_json::from_str(json).unwrap();
        assert_eq!(extract_text(&resp).unwrap(), "hello");
    }
}
