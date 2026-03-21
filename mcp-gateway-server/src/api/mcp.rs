use axum::{
    extract::State,
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::AppState;
use crate::policy::engine::{PolicyDecision, PolicyEngine};
use super::auth::Claims;

// JSON-RPC structures
#[derive(Deserialize)]
pub struct JsonRpcRequest {
    #[allow(dead_code)]
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
}

#[derive(Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Serialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

pub fn mcp_router() -> Router<AppState> {
    Router::new().route("/mcp", post(handle_mcp))
}

async fn handle_mcp(
    State(state): State<AppState>,
    claims: Claims,
    Json(req): Json<JsonRpcRequest>,
) -> Json<JsonRpcResponse> {
    let response = match req.method.as_str() {
        "initialize" => handle_initialize(&req),
        "notifications/initialized" => {
            // Notification — no response needed, but we return an ack since it's over HTTP
            JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: req.id,
                result: Some(serde_json::json!({})),
                error: None,
            }
        }
        "tools/list" => handle_tools_list(&state, &claims, &req).await,
        "tools/call" => handle_tools_call(&state, &claims, &req).await,
        _ => JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: req.id,
            result: None,
            error: Some(JsonRpcError {
                code: -32601,
                message: format!("Method not found: {}", req.method),
                data: None,
            }),
        },
    };

    Json(response)
}

fn handle_initialize(req: &JsonRpcRequest) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id: req.id.clone(),
        result: Some(serde_json::json!({
            "protocolVersion": "2025-03-26",
            "capabilities": {
                "tools": { "listChanged": false }
            },
            "serverInfo": {
                "name": "mcp-gateway",
                "version": "0.1.0"
            }
        })),
        error: None,
    }
}

async fn handle_tools_list(
    state: &AppState,
    claims: &Claims,
    req: &JsonRpcRequest,
) -> JsonRpcResponse {
    // Load all enabled tools with their backend info
    let tools: Result<Vec<(String, String, Option<String>, Option<Value>, String, Option<String>)>, _> = sqlx::query_as(
        "SELECT t.tool_name, t.original_name, t.description, t.input_schema, b.name as backend_name, t.risk_category
         FROM tool_registry t
         JOIN backends b ON t.backend_id = b.backend_id
         WHERE t.is_enabled = TRUE AND b.is_enabled = TRUE
         ORDER BY t.tool_name"
    )
    .fetch_all(&state.db)
    .await;

    let tools = match tools {
        Ok(t) => t,
        Err(e) => {
            return JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: req.id.clone(),
                result: None,
                error: Some(JsonRpcError {
                    code: -32603,
                    message: format!("Internal error: {}", e),
                    data: None,
                }),
            };
        }
    };

    // Load policy engine scoped to user's roles
    let engine = match PolicyEngine::for_roles(&state.db, &claims.roles).await {
        Ok(e) => e,
        Err(_) => PolicyEngine::new(vec![], PolicyDecision::Deny),
    };

    let mut tool_list = Vec::new();
    let mut denied_count = 0usize;
    for (tool_name, _original_name, description, input_schema, _backend_name, risk_category) in &tools {
        let tool_risk = risk_category.as_deref().unwrap_or("unclassified");
        let (decision, _, _) = engine.evaluate(tool_name, tool_risk, claims.application.as_deref());

        if decision != PolicyDecision::Allow {
            denied_count += 1;
            continue;
        }

        let mut tool_obj = serde_json::json!({
            "name": tool_name,
            "description": description.as_deref().unwrap_or(""),
        });

        if let Some(schema) = input_schema {
            tool_obj["inputSchema"] = schema.clone();
        } else {
            tool_obj["inputSchema"] = serde_json::json!({
                "type": "object",
                "properties": {}
            });
        }

        tool_list.push(tool_obj);
    }

    tracing::info!(
        user = %claims.username,
        roles = ?claims.roles,
        default_policy = %engine.default_decision(),
        total_tools = tools.len(),
        allowed = tool_list.len(),
        denied = denied_count,
        "tools/list served"
    );

    JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id: req.id.clone(),
        result: Some(serde_json::json!({ "tools": tool_list })),
        error: None,
    }
}

async fn handle_tools_call(
    state: &AppState,
    claims: &Claims,
    req: &JsonRpcRequest,
) -> JsonRpcResponse {
    let start = std::time::Instant::now();

    // Parse params
    let params = match &req.params {
        Some(p) => p,
        None => {
            return JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: req.id.clone(),
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: "Missing params".into(),
                    data: None,
                }),
            };
        }
    };

    let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let arguments = params.get("arguments").cloned().unwrap_or(serde_json::json!({}));

    if tool_name.is_empty() {
        return JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: req.id.clone(),
            result: None,
            error: Some(JsonRpcError {
                code: -32602,
                message: "Missing tool name in params".into(),
                data: None,
            }),
        };
    }

    // Resolve tool from registry
    let tool_row: Option<(Uuid, String, String, Option<String>, Uuid, String, String)> = sqlx::query_as(
        "SELECT t.tool_id, t.tool_name, t.original_name, t.risk_category, b.backend_id, b.name, b.transport
         FROM tool_registry t
         JOIN backends b ON t.backend_id = b.backend_id
         WHERE t.tool_name = $1 AND t.is_enabled = TRUE AND b.is_enabled = TRUE"
    )
    .bind(tool_name)
    .fetch_optional(&state.db)
    .await
    .unwrap_or(None);

    let (_tool_id, _tool_name, original_name, risk_category, backend_id, backend_name, transport) = match tool_row {
        Some(r) => r,
        None => {
            return JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: req.id.clone(),
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: format!("Tool not found: {}", tool_name),
                    data: None,
                }),
            };
        }
    };

    let risk = risk_category.as_deref().unwrap_or("read");
    let user_id: Option<Uuid> = claims.sub.parse().ok();

    // Load policies scoped to user's roles and evaluate
    let engine = match PolicyEngine::for_roles(&state.db, &claims.roles).await {
        Ok(e) => e,
        Err(_) => PolicyEngine::new(vec![], PolicyDecision::Deny),
    };

    let (decision, policy_id, reason) = engine.evaluate(tool_name, risk, claims.application.as_deref());
    let decision_str = decision.to_string();

    // Record metrics
    state.metrics.record_policy_decision(&decision_str, tool_name);

    if decision != PolicyDecision::Allow {
        let duration = start.elapsed();
        let duration_ms = duration.as_secs_f64() * 1000.0;

        // Audit the denial
        if let Some(ref audit) = state.audit {
            let _ = audit.record_event(
                tool_name,
                &backend_name,
                risk,
                Some(&arguments.to_string()),
                None,
                duration_ms,
                "denied",
                reason.as_deref(),
                "deny",
                policy_id.as_deref(),
                user_id,
                None,
                None,
                claims.application.as_deref(),
            ).await;
        }

        state.metrics.record_tool_call(tool_name, &backend_name, "denied", risk, duration.as_secs_f64());

        let deny_reason = reason.unwrap_or_else(|| "Access denied by policy".into());
        return JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: req.id.clone(),
            result: None,
            error: Some(JsonRpcError {
                code: -32603,
                message: format!("Policy denied: {}", deny_reason),
                data: policy_id.map(|id| serde_json::json!({ "policy_id": id })),
            }),
        };
    }

    // Forward to backend — returns the raw MCP result object (preserving isError, content, etc.)
    let result = match transport.as_str() {
        "streamable-http" => {
            let config_row: Option<(serde_json::Value,)> = sqlx::query_as(
                "SELECT config FROM backends WHERE backend_id = $1"
            ).bind(backend_id).fetch_optional(&state.db).await.unwrap_or(None);

            match config_row {
                Some((config,)) => {
                    crate::backends::BackendManager::call_http_tool(&config, &original_name, &arguments).await
                }
                None => Err("Backend config not found".into()),
            }
        }
        "sse" => {
            let config_row: Option<(serde_json::Value,)> = sqlx::query_as(
                "SELECT config FROM backends WHERE backend_id = $1"
            ).bind(backend_id).fetch_optional(&state.db).await.unwrap_or(None);

            match config_row {
                Some((config,)) => {
                    crate::backends::BackendManager::call_sse_tool(&config, &original_name, &arguments).await
                }
                None => Err("Backend config not found".into()),
            }
        }
        "stdio" => {
            state.backend_manager.call_tool(&backend_id, &original_name, &arguments).await
        }
        "agent" => {
            let config_row: Option<(serde_json::Value,)> = sqlx::query_as(
                "SELECT config FROM backends WHERE backend_id = $1"
            ).bind(backend_id).fetch_optional(&state.db).await.unwrap_or(None);

            match config_row {
                Some((config,)) => {
                    let agent_id = config.get("agent_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or(&backend_name);
                    state.agent_registry
                        .call_tool(agent_id, &original_name, &arguments)
                        .await
                }
                None => Err("Backend config not found".into()),
            }
        }
        _ => {
            Err(format!(
                "Backend '{}' uses unsupported transport: {}",
                backend_name, transport
            ))
        }
    };

    let duration = start.elapsed();
    let duration_ms = duration.as_secs_f64() * 1000.0;

    match result {
        Ok(raw_result) => {
            // Check if the backend signalled a tool-level error via isError flag
            let is_tool_error = raw_result.get("isError")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            // Extract content from the result, preserving the isError flag
            let content = if let Some(c) = raw_result.get("content") {
                c.clone()
            } else {
                raw_result.clone()
            };

            let audit_status = if is_tool_error { "tool_error" } else { "success" };

            // Audit
            if let Some(ref audit) = state.audit {
                let _ = audit.record_event(
                    tool_name,
                    &backend_name,
                    risk,
                    Some(&arguments.to_string()),
                    Some(&content.to_string()),
                    duration_ms,
                    audit_status,
                    None,
                    &decision_str,
                    policy_id.as_deref(),
                    user_id,
                    None,
                    None,
                    claims.application.as_deref(),
                ).await;
            }

            state.metrics.record_tool_call(tool_name, &backend_name, audit_status, risk, duration.as_secs_f64());

            // If the backend already returned MCP content array, pass it through directly
            if content.is_array() {
                if let Some(first) = content.as_array().and_then(|a| a.first()) {
                    if first.get("type").is_some() {
                        let mut result_obj = serde_json::json!({ "content": content });
                        if is_tool_error {
                            result_obj["isError"] = Value::Bool(true);
                        }
                        return JsonRpcResponse {
                            jsonrpc: "2.0".into(),
                            id: req.id.clone(),
                            result: Some(result_obj),
                            error: None,
                        };
                    }
                }
            }

            // Extract text: use the string value directly if it's a string, otherwise serialize
            let text = match content {
                Value::String(s) => s,
                other => other.to_string(),
            };

            let mut result_obj = serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": text
                }]
            });
            if is_tool_error {
                result_obj["isError"] = Value::Bool(true);
            }

            JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: req.id.clone(),
                result: Some(result_obj),
                error: None,
            }
        }
        Err(err_msg) => {
            // Audit error
            if let Some(ref audit) = state.audit {
                let _ = audit.record_event(
                    tool_name,
                    &backend_name,
                    risk,
                    Some(&arguments.to_string()),
                    None,
                    duration_ms,
                    "error",
                    Some(&err_msg),
                    &decision_str,
                    policy_id.as_deref(),
                    user_id,
                    None,
                    None,
                    claims.application.as_deref(),
                ).await;
            }

            state.metrics.record_tool_call(tool_name, &backend_name, "error", risk, duration.as_secs_f64());

            JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: req.id.clone(),
                result: Some(serde_json::json!({
                    "content": [{
                        "type": "text",
                        "text": err_msg
                    }],
                    "isError": true
                })),
                error: None,
            }
        }
    }
}

