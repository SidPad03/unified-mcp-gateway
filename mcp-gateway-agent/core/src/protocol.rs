//! The agent ↔ gateway wire protocol.
//!
//! These enums are duplicated in `mcp-gateway-server/src/agent/mod.rs`. Two
//! crates in two build graphs cannot share a type, so the guard against drift is
//! the golden-JSON tests at the bottom of this file: they pin the exact bytes of
//! every message. If a field is renamed on either side, one of them fails.
//!
//! Rules that hold across the whole protocol:
//!
//! * Messages are JSON text frames, one message per frame.
//! * The variant tag is `type`, snake_case.
//! * `inputSchema` is camelCase because it comes straight from MCP's
//!   `tools/list`; everything else is snake_case.
//! * Tool names are namespaced `<backend>__<tool>`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Agent → gateway.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum AgentMessage {
    #[serde(rename = "register")]
    Register {
        agent_id: String,
        tools: Vec<ToolInfo>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        backends: Vec<SubBackendInfo>,
    },
    #[serde(rename = "tool_result")]
    ToolResult { request_id: String, result: Value },
    #[serde(rename = "tool_error")]
    ToolError { request_id: String, error: String },
    #[serde(rename = "ping")]
    Ping,
}

/// Gateway → agent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum GatewayMessage {
    #[serde(rename = "registered")]
    Registered { backend_id: String },
    #[serde(rename = "tool_call")]
    ToolCall {
        request_id: String,
        tool: String,
        arguments: Value,
    },
    #[serde(rename = "pong")]
    Pong,
    #[serde(rename = "resync")]
    Resync,
    #[serde(rename = "error")]
    Error { message: String },
}

/// One tool, as the gateway stores it in `tool_registry`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

/// Metadata about one local MCP server behind this agent, so the dashboard can
/// show the tree rather than a flat list of tools.
///
/// Only **key names** of the environment are sent — the values are the whole
/// reason a backend has an `env` block in the first place.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SubBackendInfo {
    pub name: String,
    pub transport: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env_keys: Vec<String>,
    #[serde(default)]
    pub tool_count: usize,
}

impl AgentMessage {
    pub fn to_frame(&self) -> String {
        // Every variant is plain data; serialization cannot fail.
        serde_json::to_string(self).expect("agent message is serializable")
    }
}

#[cfg(test)]
mod golden {
    //! Byte-exact expectations. Changing any of these means changing the
    //! server's `agent/mod.rs` in the same commit.

    use super::*;
    use serde_json::json;

    fn frame(msg: &AgentMessage) -> Value {
        serde_json::from_str(&msg.to_frame()).unwrap()
    }

    #[test]
    fn register_frame() {
        let msg = AgentMessage::Register {
            agent_id: "sids-macbook-pro".into(),
            tools: vec![ToolInfo {
                name: "blender__get_scene_info".into(),
                description: "Get scene info".into(),
                input_schema: json!({"type": "object", "properties": {}}),
            }],
            backends: vec![SubBackendInfo {
                name: "blender".into(),
                transport: "stdio".into(),
                command: Some("uvx".into()),
                args: vec!["blender-mcp".into()],
                url: None,
                env_keys: vec!["BLENDER_PATH".into()],
                tool_count: 1,
            }],
        };
        assert_eq!(
            frame(&msg),
            json!({
                "type": "register",
                "agent_id": "sids-macbook-pro",
                "tools": [{
                    "name": "blender__get_scene_info",
                    "description": "Get scene info",
                    "inputSchema": {"type": "object", "properties": {}}
                }],
                "backends": [{
                    "name": "blender",
                    "transport": "stdio",
                    "command": "uvx",
                    "args": ["blender-mcp"],
                    "env_keys": ["BLENDER_PATH"],
                    "tool_count": 1
                }]
            })
        );
    }

    #[test]
    fn register_with_no_backends_omits_the_field() {
        let msg = AgentMessage::Register {
            agent_id: "a".into(),
            tools: vec![],
            backends: vec![],
        };
        assert_eq!(
            frame(&msg),
            json!({"type": "register", "agent_id": "a", "tools": []})
        );
    }

    #[test]
    fn tool_result_frame() {
        let msg = AgentMessage::ToolResult {
            request_id: "req-1".into(),
            result: json!({"content": [{"type": "text", "text": "ok"}]}),
        };
        assert_eq!(
            frame(&msg),
            json!({
                "type": "tool_result",
                "request_id": "req-1",
                "result": {"content": [{"type": "text", "text": "ok"}]}
            })
        );
    }

    #[test]
    fn tool_error_frame() {
        let msg = AgentMessage::ToolError {
            request_id: "req-2".into(),
            error: "Backend 'blender' is not running".into(),
        };
        assert_eq!(
            frame(&msg),
            json!({
                "type": "tool_error",
                "request_id": "req-2",
                "error": "Backend 'blender' is not running"
            })
        );
    }

    #[test]
    fn ping_frame() {
        assert_eq!(frame(&AgentMessage::Ping), json!({"type": "ping"}));
    }

    #[test]
    fn parses_every_gateway_message() {
        let cases = [
            (
                r#"{"type":"registered","backend_id":"agent-sids-macbook-pro"}"#,
                GatewayMessage::Registered {
                    backend_id: "agent-sids-macbook-pro".into(),
                },
            ),
            (
                r#"{"type":"tool_call","request_id":"r1","tool":"blender__ping","arguments":{"a":1}}"#,
                GatewayMessage::ToolCall {
                    request_id: "r1".into(),
                    tool: "blender__ping".into(),
                    arguments: json!({"a": 1}),
                },
            ),
            (r#"{"type":"pong"}"#, GatewayMessage::Pong),
            (r#"{"type":"resync"}"#, GatewayMessage::Resync),
            (
                r#"{"type":"error","message":"unknown agent"}"#,
                GatewayMessage::Error {
                    message: "unknown agent".into(),
                },
            ),
        ];
        for (raw, expected) in cases {
            let parsed: GatewayMessage = serde_json::from_str(raw).unwrap_or_else(|e| {
                panic!("failed to parse {raw}: {e}");
            });
            assert_eq!(parsed, expected);
        }
    }

    #[test]
    fn unknown_gateway_message_is_rejected_not_guessed() {
        // The tunnel logs and skips these rather than treating them as a variant
        // it happens to structurally match.
        assert!(serde_json::from_str::<GatewayMessage>(r#"{"type":"teapot"}"#).is_err());
    }

    #[test]
    fn a_sub_backend_never_carries_env_values() {
        let info = SubBackendInfo {
            name: "gitea".into(),
            transport: "stdio".into(),
            command: Some("gitea-mcp".into()),
            args: vec![],
            url: None,
            env_keys: vec!["GITEA_TOKEN".into()],
            tool_count: 12,
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("GITEA_TOKEN"));
        assert!(
            !json.contains("env\":{"),
            "sub-backend info must not carry env values: {json}"
        );
    }
}
