use axum::{extract::State, routing::post, Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::auth::Claims;
use crate::policy::engine::{PolicyDecision, PolicyEngine};
use crate::AppState;

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

/// The risk label to evaluate policies against when a `tool_registry` row has
/// no `risk_category`.
///
/// `tools/list` and `tools/call` MUST agree on this. They used to disagree —
/// list said "unclassified", call said "read" — so a policy denying the
/// `unclassified` category hid a NULL-risk tool from discovery while still
/// permitting a direct `tools/call`, which evaluated as "read" and fell
/// through to the role's default allow.
///
/// NULL is reachable and durable: rows created before auto-classification keep
/// it, because the discovery upsert deliberately never overwrites a stored
/// `risk_category` (see `crate::register_discovered_tools`). "unclassified" is
/// the value the classifier, the dashboard, and the policy editor all use, and
/// treating an unreviewed tool as the *lowest* risk is the wrong direction for
/// a gateway to fail.
pub(crate) const DEFAULT_RISK: &str = "unclassified";

fn effective_risk(risk_category: Option<&str>) -> &str {
    risk_category.unwrap_or(DEFAULT_RISK)
}

/// Where a `tools/call` is headed.
///
/// Both arms take the same road — policy, audit, metrics — and differ only in
/// who actually runs the call. Keeping that road single is the point: a
/// `gateway_*` call that skipped the audit trail would be the one kind of call
/// worth hiding.
enum Target {
    Backend {
        original_name: String,
        risk_category: Option<String>,
        backend_id: Uuid,
        backend_name: String,
        transport: String,
    },
    Gateway {
        risk: &'static str,
    },
}

impl Target {
    fn risk(&self) -> &str {
        match self {
            Target::Backend { risk_category, .. } => effective_risk(risk_category.as_deref()),
            Target::Gateway { risk } => risk,
        }
    }

    /// Whether this call is the gateway configuring itself rather than
    /// somebody using the gateway.
    ///
    /// An internal call is still resolved, still policy-evaluated and still
    /// role-gated — it simply leaves no trace on the surfaces that are about
    /// the operator's own traffic: no audit row, no Prometheus sample, nothing
    /// on the live feed, nothing in the usage graph. The gateway is for the
    /// tools its operator put behind it; its own plumbing is not part of that
    /// picture, and a burst of `gateway_*` calls in the middle of an audit
    /// trail is noise in the one place that has to stay readable.
    ///
    /// The calls are not *invisible*: each one is written to the server log at
    /// INFO, so `docker logs` still answers "who reconfigured this and when".
    fn is_internal(&self) -> bool {
        match self {
            Target::Gateway { .. } => true,
            Target::Backend { original_name, .. } => {
                crate::backends::classifier::is_control_tool(original_name)
            }
        }
    }

    /// What the audit trail files the call under. The gateway's own tools are
    /// filed under `gateway`, which is not a name a backend can take — the
    /// `backends` table has a unique name and nothing registers itself there.
    fn backend_name(&self) -> &str {
        match self {
            Target::Backend { backend_name, .. } => backend_name,
            Target::Gateway { .. } => "gateway",
        }
    }
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
            tracing::error!(error = %e, "Failed to load tools for tools/list");
            return JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: req.id.clone(),
                result: None,
                error: Some(JsonRpcError {
                    code: -32603,
                    message: "Internal error".into(),
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
    for (tool_name, _original_name, description, input_schema, _backend_name, risk_category) in
        &tools
    {
        let tool_risk = effective_risk(risk_category.as_deref());
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

    // The gateway's own tools, offered alongside everything it routes. They go
    // through the same policy engine — they are classified on the same ladder —
    // and additionally past the role gate, because advertising a tool that will
    // always be refused only wastes the reader's context.
    let mut gateway_count = 0usize;
    let gateway_tools_enabled = crate::api::settings::read_bool(
        &state.db,
        crate::api::settings::GATEWAY_TOOLS_ENABLED,
        true,
    )
    .await;
    for tool in crate::gateway_tools::catalog() {
        if !gateway_tools_enabled {
            break;
        }
        if !crate::gateway_tools::may_call(&tool, claims) {
            continue;
        }
        let (decision, _, _) = engine.evaluate(tool.name, tool.risk, claims.application.as_deref());
        if decision != PolicyDecision::Allow {
            denied_count += 1;
            continue;
        }
        // A backend tool is always `<backend>__<tool>`, so this can only fire
        // if someone adds a catalog entry that collides. Say so rather than
        // silently serving two tools with one name.
        if tools.iter().any(|(name, ..)| name == tool.name) {
            tracing::warn!(
                tool = tool.name,
                "A registered tool shadows a gateway tool name; the gateway tool wins"
            );
        }
        tool_list.push(serde_json::json!({
            "name": tool.name,
            "description": tool.description,
            "inputSchema": tool.input_schema,
        }));
        gateway_count += 1;
    }

    tracing::info!(
        user = %claims.username,
        roles = ?claims.roles,
        default_policy = %engine.default_decision(),
        total_tools = tools.len(),
        allowed = tool_list.len(),
        gateway_tools = gateway_count,
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
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or(serde_json::json!({}));

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

    // Where the call is going. The gateway's own tools are matched by exact
    // name and checked first; a backend tool is always `<backend>__<tool>`, so
    // the two namespaces cannot overlap.
    let gateway_tool = crate::gateway_tools::find(tool_name);

    let target = match &gateway_tool {
        Some(def) => Target::Gateway { risk: def.risk },
        None => {
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

            match tool_row {
                Some((_, _, original_name, risk_category, backend_id, backend_name, transport)) => {
                    Target::Backend {
                        original_name,
                        risk_category,
                        backend_id,
                        backend_name,
                        transport,
                    }
                }
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
            }
        }
    };

    // Switched off in Settings, the namespace is not there at all: a call to it
    // is answered the same way a call to any unknown tool is, rather than as a
    // policy refusal, because "the gateway does not offer this" is the truth.
    if gateway_tool.is_some()
        && !crate::api::settings::read_bool(
            &state.db,
            crate::api::settings::GATEWAY_TOOLS_ENABLED,
            true,
        )
        .await
    {
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

    let risk = target.risk();
    let backend_name = target.backend_name().to_string();
    let internal = target.is_internal();
    let user_id: Option<Uuid> = claims.sub.parse().ok();

    // Load policies scoped to user's roles and evaluate
    let engine = match PolicyEngine::for_roles(&state.db, &claims.roles).await {
        Ok(e) => e,
        Err(_) => PolicyEngine::new(vec![], PolicyDecision::Deny),
    };

    let (mut decision, policy_id, mut reason) =
        engine.evaluate(tool_name, risk, claims.application.as_deref());

    // The role gate on the gateway's own tools sits on top of policy, and is
    // recorded as a denial rather than a failed call: refusing to configure the
    // gateway is a policy outcome, and belongs in the audit trail as one.
    if let Some(def) = &gateway_tool {
        if !crate::gateway_tools::may_call(def, claims) {
            decision = PolicyDecision::Deny;
            reason = Some(format!(
                "'{}' configures the gateway and requires the owner role",
                def.name
            ));
        }
    }
    let decision_str = decision.to_string();

    // Record metrics
    if !internal {
        state
            .metrics
            .record_policy_decision(&decision_str, tool_name);
    }

    if decision != PolicyDecision::Allow {
        let duration = start.elapsed();
        let duration_ms = duration.as_secs_f64() * 1000.0;

        if internal {
            // The refusal still has to be findable by whoever runs the
            // gateway, just not on the pages that are about their traffic.
            tracing::warn!(
                user = %claims.username,
                tool = tool_name,
                reason = reason.as_deref().unwrap_or("policy"),
                "Refused an internal gateway tool call"
            );
        } else {
            // Audit the denial
            if let Some(ref audit) = state.audit {
                let _ = audit
                    .record_event(
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
                    )
                    .await;
            }

            state.metrics.record_tool_call(
                tool_name,
                &backend_name,
                "denied",
                risk,
                duration.as_secs_f64(),
            );
        }

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

    // Run the call. A gateway tool runs here, in-process; everything else is
    // forwarded and comes back as the raw MCP result object (preserving
    // isError, content, and so on).
    let result = match &target {
        Target::Gateway { .. } => {
            crate::gateway_tools::call(state, claims, tool_name, &arguments).await
        }
        Target::Backend {
            original_name,
            backend_id,
            transport,
            ..
        } => match transport.as_str() {
            "streamable-http" => {
                let config_row: Option<(serde_json::Value,)> =
                    sqlx::query_as("SELECT config FROM backends WHERE backend_id = $1")
                        .bind(*backend_id)
                        .fetch_optional(&state.db)
                        .await
                        .unwrap_or(None);

                match config_row {
                    Some((config,)) => {
                        crate::backends::BackendManager::call_http_tool(
                            &config,
                            original_name,
                            &arguments,
                        )
                        .await
                    }
                    None => Err("Backend config not found".into()),
                }
            }
            "sse" => {
                let config_row: Option<(serde_json::Value,)> =
                    sqlx::query_as("SELECT config FROM backends WHERE backend_id = $1")
                        .bind(*backend_id)
                        .fetch_optional(&state.db)
                        .await
                        .unwrap_or(None);

                match config_row {
                    Some((config,)) => {
                        crate::backends::BackendManager::call_sse_tool(
                            &config,
                            original_name,
                            &arguments,
                        )
                        .await
                    }
                    None => Err("Backend config not found".into()),
                }
            }
            "stdio" => {
                state
                    .backend_manager
                    .call_tool(backend_id, original_name, &arguments)
                    .await
            }
            "agent" => {
                let config_row: Option<(serde_json::Value,)> =
                    sqlx::query_as("SELECT config FROM backends WHERE backend_id = $1")
                        .bind(*backend_id)
                        .fetch_optional(&state.db)
                        .await
                        .unwrap_or(None);

                match config_row {
                    Some((config,)) => {
                        let agent_id = config
                            .get("agent_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or(&backend_name);
                        state
                            .agent_registry
                            .call_tool(agent_id, original_name, &arguments)
                            .await
                    }
                    None => Err("Backend config not found".into()),
                }
            }
            _ => Err(format!(
                "Backend '{}' uses unsupported transport: {}",
                backend_name, transport
            )),
        },
    };

    let duration = start.elapsed();
    let duration_ms = duration.as_secs_f64() * 1000.0;

    match result {
        Ok(raw_result) => {
            // Check if the backend signalled a tool-level error via isError flag
            let is_tool_error = raw_result
                .get("isError")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            // Extract content from the result, preserving the isError flag
            let content = if let Some(c) = raw_result.get("content") {
                c.clone()
            } else {
                raw_result.clone()
            };

            let audit_status = if is_tool_error {
                "tool_error"
            } else {
                "success"
            };

            if internal {
                tracing::info!(
                    user = %claims.username,
                    tool = tool_name,
                    status = audit_status,
                    duration_ms,
                    "Internal gateway tool call"
                );
            } else {
                // Audit
                if let Some(ref audit) = state.audit {
                    let _ = audit
                        .record_event(
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
                        )
                        .await;
                }

                state.metrics.record_tool_call(
                    tool_name,
                    &backend_name,
                    audit_status,
                    risk,
                    duration.as_secs_f64(),
                );
            }

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
            if internal {
                tracing::warn!(
                    user = %claims.username,
                    tool = tool_name,
                    error = %err_msg,
                    "Internal gateway tool call failed"
                );
            } else {
                // Audit error
                if let Some(ref audit) = state.audit {
                    let _ = audit
                        .record_event(
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
                        )
                        .await;
                }

                state.metrics.record_tool_call(
                    tool_name,
                    &backend_name,
                    "error",
                    risk,
                    duration.as_secs_f64(),
                );
            }

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::engine::PolicyRule;

    fn backend_target(original_name: &str) -> Target {
        Target::Backend {
            original_name: original_name.into(),
            risk_category: Some("read".into()),
            backend_id: Uuid::nil(),
            backend_name: "mac".into(),
            transport: "agent".into(),
        }
    }

    /// The gateway's own tools are plumbing, and so are the control tools an
    /// agent registers. Neither belongs in the audit trail, the metrics or the
    /// usage graph, which are about the traffic the operator's own tools carry.
    #[test]
    fn the_gateways_own_plumbing_is_internal() {
        assert!(Target::Gateway { risk: "admin" }.is_internal());
        assert!(backend_target("agent_install_mcp_server").is_internal());
        assert!(backend_target("agent_get_local_server_logs").is_internal());
    }

    /// Everything the operator actually put behind the gateway is not, however
    /// close its name gets.
    #[test]
    fn a_users_own_tool_is_never_internal() {
        assert!(!backend_target("obsidian_list_notes").is_internal());
        assert!(!backend_target("agent_install_mcp_server_v2").is_internal());
        assert!(!backend_target("delete_branch").is_internal());
    }

    /// `gateway` is what an internal call would be filed under if it were ever
    /// recorded, and it is not a name a backend can take — `backends.name` is
    /// unique and nothing registers itself there.
    #[test]
    fn a_gateway_call_is_filed_under_the_gateway() {
        assert_eq!(Target::Gateway { risk: "read" }.backend_name(), "gateway");
        assert_eq!(backend_target("x").backend_name(), "mac");
    }

    #[test]
    fn null_risk_resolves_to_unclassified() {
        assert_eq!(effective_risk(None), DEFAULT_RISK);
        assert_eq!(effective_risk(None), "unclassified");
        // A stored category is passed through untouched.
        assert_eq!(effective_risk(Some("destructive")), "destructive");
        assert_eq!(effective_risk(Some("read")), "read");
    }

    /// Regression test for a policy bypass.
    ///
    /// `tools/call` mapped a NULL `risk_category` to "read" while `tools/list`
    /// mapped it to "unclassified". An operator who wrote "deny anything still
    /// unclassified" saw the tool disappear from discovery and reasonably
    /// concluded it was blocked — but a direct `tools/call` evaluated it as
    /// "read", missed the deny rule, and fell through to the role's default
    /// allow (`owner` defaults to allow). Both paths now share
    /// [`effective_risk`], so they cannot diverge again.
    #[test]
    fn deny_unclassified_policy_also_blocks_a_null_risk_tool() {
        let engine = PolicyEngine::new(
            vec![PolicyRule {
                policy_id: "00000000-0000-0000-0000-000000000001".into(),
                name: "Deny unclassified tools".into(),
                priority: 1,
                tool_pattern: "*".into(),
                decision: PolicyDecision::Deny,
                reason: Some("tool has not been risk-reviewed".into()),
                risk_categories: vec!["unclassified".into()],
                application_match: None,
            }],
            // The permissive default that made the bypass reachable.
            PolicyDecision::Allow,
        );

        // What tools/call now evaluates for a NULL-risk tool.
        let (decision, _, _) = engine.evaluate("srv__legacy_tool", effective_risk(None), None);
        assert_eq!(
            decision,
            PolicyDecision::Deny,
            "a NULL-risk tool must be denied by a deny-unclassified policy"
        );

        // The pre-fix mapping, kept explicit: evaluating the same tool as
        // "read" slips past the rule entirely. This is what made it a bypass.
        let (as_read, _, _) = engine.evaluate("srv__legacy_tool", "read", None);
        assert_eq!(
            as_read,
            PolicyDecision::Allow,
            "sanity check: the deny rule is category-scoped, so 'read' does not match it"
        );

        // And tools/list agrees with tools/call, which is the whole point.
        let (listed, _, _) = engine.evaluate("srv__legacy_tool", effective_risk(None), None);
        assert_eq!(
            listed, decision,
            "list and call must reach the same verdict"
        );
    }
}
