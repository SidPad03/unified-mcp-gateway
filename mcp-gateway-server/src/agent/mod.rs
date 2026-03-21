use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    http::HeaderMap,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{mpsc, oneshot, Mutex, RwLock};
use uuid::Uuid;

use crate::AppState;

// ── Wire protocol ───────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AgentMessage {
    #[serde(rename = "register")]
    Register {
        agent_id: String,
        tools: Vec<AgentToolInfo>,
        #[serde(default)]
        backends: Vec<AgentSubBackendInfo>,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        request_id: String,
        result: Value,
    },
    #[serde(rename = "tool_error")]
    ToolError {
        request_id: String,
        error: String,
    },
    #[serde(rename = "ping")]
    Ping,
}

#[derive(Debug, Serialize, Deserialize)]
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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AgentToolInfo {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AgentSubBackendInfo {
    pub name: String,
    pub transport: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub args: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub env_keys: Vec<String>,
    #[serde(default)]
    pub tool_count: usize,
}

// ── Agent registry ──────────────────────────────────────────────────────

pub struct AgentEnvelope {
    pub request_id: String,
    pub tool_name: String,
    pub arguments: Value,
    pub response_tx: oneshot::Sender<Result<Value, String>>,
}

struct AgentHandle {
    tx: mpsc::Sender<AgentEnvelope>,
    control_tx: mpsc::Sender<AgentControl>,
}

pub enum AgentControl {
    Resync,
}

pub struct AgentRegistry {
    agents: RwLock<HashMap<String, AgentHandle>>,
}

impl AgentRegistry {
    pub fn new() -> Self {
        Self {
            agents: RwLock::new(HashMap::new()),
        }
    }

    /// Forward a tool call to a connected agent and wait for the response.
    pub async fn call_tool(
        &self,
        agent_id: &str,
        tool_name: &str,
        arguments: &Value,
    ) -> Result<Value, String> {
        let agents = self.agents.read().await;
        let handle = agents
            .get(agent_id)
            .ok_or_else(|| format!("Agent '{}' not connected", agent_id))?;

        let (response_tx, response_rx) = oneshot::channel();
        let envelope = AgentEnvelope {
            request_id: Uuid::new_v4().to_string(),
            tool_name: tool_name.to_string(),
            arguments: arguments.clone(),
            response_tx,
        };

        handle
            .tx
            .send(envelope)
            .await
            .map_err(|_| format!("Agent '{}' connection closed", agent_id))?;

        tokio::time::timeout(std::time::Duration::from_secs(120), response_rx)
            .await
            .map_err(|_| format!("Timeout (120s) waiting for agent '{}' response", agent_id))?
            .map_err(|_| format!("Agent '{}' dropped the response channel", agent_id))?
    }

    async fn register(
        &self,
        agent_id: String,
        tx: mpsc::Sender<AgentEnvelope>,
        control_tx: mpsc::Sender<AgentControl>,
    ) {
        tracing::info!(agent_id = %agent_id, "Agent registered in memory");
        self.agents
            .write()
            .await
            .insert(agent_id, AgentHandle { tx, control_tx });
    }

    /// Ask a connected agent to re-send its registration (tool list refresh).
    pub async fn request_resync(&self, agent_id: &str) -> Result<(), String> {
        let agents = self.agents.read().await;
        let handle = agents
            .get(agent_id)
            .ok_or_else(|| format!("Agent '{}' not connected", agent_id))?;
        handle
            .control_tx
            .send(AgentControl::Resync)
            .await
            .map_err(|_| format!("Agent '{}' connection closed", agent_id))
    }

    async fn unregister(&self, agent_id: &str) {
        tracing::info!(agent_id = %agent_id, "Agent unregistered from memory");
        self.agents.write().await.remove(agent_id);
    }

    pub async fn is_connected(&self, agent_id: &str) -> bool {
        self.agents.read().await.contains_key(agent_id)
    }
}

// ── WebSocket endpoint ──────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct WsAuthQuery {
    pub token: Option<String>,
}

pub async fn agent_ws_handler(
    State(state): State<AppState>,
    Query(query): Query<WsAuthQuery>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    let token = query.token.or_else(|| {
        headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(|s| s.to_string())
    });

    match token {
        Some(t) => ws.on_upgrade(move |socket| handle_agent_connection(state, socket, t)),
        None => ws.on_upgrade(|socket| async {
            let mut socket = socket;
            let msg = GatewayMessage::Error {
                message: "Missing authentication token. Provide ?token= query param or Authorization header.".into(),
            };
            let _ = socket
                .send(Message::Text(serde_json::to_string(&msg).unwrap()))
                .await;
            let _ = socket.close().await;
        }),
    }
}

async fn handle_agent_connection(state: AppState, mut socket: WebSocket, token: String) {
    // Authenticate
    match crate::api::auth::resolve_api_key(&token, &state).await {
        Ok(_claims) => {}
        Err(e) => {
            let err_msg = match e {
                crate::AppError::Unauthorized(msg) => msg,
                crate::AppError::Forbidden(msg) => msg,
                _ => "Authentication failed".to_string(),
            };
            tracing::warn!("Agent WebSocket auth failed: {}", err_msg);
            let msg = GatewayMessage::Error { message: err_msg };
            let _ = socket
                .send(Message::Text(serde_json::to_string(&msg).unwrap()))
                .await;
            let _ = socket.close().await;
            return;
        }
    }

    // Wait for register message (30s timeout)
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(30);
    let (agent_id, tools, sub_backends) = loop {
        match tokio::time::timeout_at(deadline, socket.recv()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                match serde_json::from_str::<AgentMessage>(&text) {
                    Ok(AgentMessage::Register { agent_id, tools, backends }) => break (agent_id, tools, backends),
                    Ok(_) => continue,
                    Err(e) => {
                        tracing::debug!(error = %e, "Ignoring non-register message");
                        continue;
                    }
                }
            }
            Ok(Some(Ok(_))) => continue,
            _ => {
                tracing::warn!("Agent failed to send register message within 30s");
                let _ = socket.close().await;
                return;
            }
        }
    };

    tracing::info!(agent_id = %agent_id, tool_count = tools.len(), "Agent registering");

    // Register backend + tools in DB
    let backend_id = match register_agent_in_db(&state, &agent_id, &tools, &sub_backends).await {
        Ok(id) => id,
        Err(e) => {
            tracing::error!(agent_id = %agent_id, error = %e, "Failed to register agent in DB");
            let msg = GatewayMessage::Error { message: e };
            let _ = socket
                .send(Message::Text(serde_json::to_string(&msg).unwrap()))
                .await;
            let _ = socket.close().await;
            return;
        }
    };

    // Confirm registration
    let confirm = GatewayMessage::Registered {
        backend_id: backend_id.to_string(),
    };
    if socket
        .send(Message::Text(serde_json::to_string(&confirm).unwrap()))
        .await
        .is_err()
    {
        return;
    }

    // Set up request forwarding
    let (request_tx, mut request_rx) = mpsc::channel::<AgentEnvelope>(64);
    let (control_tx, mut control_rx) = mpsc::channel::<AgentControl>(8);
    let pending: Arc<Mutex<HashMap<String, oneshot::Sender<Result<Value, String>>>>> =
        Arc::new(Mutex::new(HashMap::new()));

    state
        .agent_registry
        .register(agent_id.clone(), request_tx, control_tx)
        .await;

    tracing::info!(agent_id = %agent_id, backend_id = %backend_id, "Agent connected and ready");

    // Main event loop
    let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(30));
    heartbeat.tick().await; // consume the immediate first tick

    loop {
        tokio::select! {
            // ── Incoming from agent ─────────────────────────────────
            ws_msg = socket.recv() => {
                match ws_msg {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<AgentMessage>(&text) {
                            Ok(AgentMessage::ToolResult { request_id, result }) => {
                                if let Some(tx) = pending.lock().await.remove(&request_id) {
                                    let _ = tx.send(Ok(result));
                                }
                            }
                            Ok(AgentMessage::ToolError { request_id, error }) => {
                                if let Some(tx) = pending.lock().await.remove(&request_id) {
                                    let _ = tx.send(Err(error));
                                }
                            }
                            Ok(AgentMessage::Ping) => {
                                let pong = serde_json::to_string(&GatewayMessage::Pong).unwrap();
                                if socket.send(Message::Text(pong)).await.is_err() {
                                    break;
                                }
                            }
                            Ok(AgentMessage::Register { agent_id: new_id, tools: new_tools, backends: new_backends }) => {
                                tracing::info!(agent_id = %new_id, tool_count = new_tools.len(), "Agent re-registering tools");
                                let _ = register_agent_in_db(&state, &new_id, &new_tools, &new_backends).await;
                            }
                            Err(e) => {
                                tracing::debug!(error = %e, "Ignoring unparseable agent message");
                            }
                        }
                    }
                    Some(Ok(Message::Ping(data))) => {
                        if socket.send(Message::Pong(data)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(e)) => {
                        tracing::warn!(agent_id = %agent_id, error = %e, "WebSocket read error");
                        break;
                    }
                }
            }

            // ── Outgoing tool call to agent ─────────────────────────
            Some(envelope) = request_rx.recv() => {
                pending.lock().await.insert(
                    envelope.request_id.clone(),
                    envelope.response_tx,
                );
                let msg = GatewayMessage::ToolCall {
                    request_id: envelope.request_id,
                    tool: envelope.tool_name,
                    arguments: envelope.arguments,
                };
                let text = serde_json::to_string(&msg).unwrap();
                if socket.send(Message::Text(text)).await.is_err() {
                    break;
                }
            }

            // ── Control messages (e.g., resync from dashboard) ────
            Some(ctrl) = control_rx.recv() => {
                match ctrl {
                    AgentControl::Resync => {
                        tracing::info!(agent_id = %agent_id, "Sending resync request to agent");
                        let msg = serde_json::to_string(&GatewayMessage::Resync).unwrap();
                        if socket.send(Message::Text(msg)).await.is_err() {
                            break;
                        }
                    }
                }
            }

            // ── Heartbeat ───────────────────────────────────────────
            _ = heartbeat.tick() => {
                if socket.send(Message::Ping(vec![])).await.is_err() {
                    break;
                }
            }
        }
    }

    // ── Cleanup ─────────────────────────────────────────────────────────

    state.agent_registry.unregister(&agent_id).await;

    let _ = sqlx::query(
        "UPDATE backends SET health_status = 'disconnected', last_health_check = NOW() WHERE backend_id = $1",
    )
    .bind(backend_id)
    .execute(&state.db)
    .await;

    // Fail all pending requests
    let mut pending_map = pending.lock().await;
    for (_, tx) in pending_map.drain() {
        let _ = tx.send(Err("Agent disconnected".into()));
    }

    tracing::info!(agent_id = %agent_id, "Agent disconnected and cleaned up");
}

// ── DB helpers ──────────────────────────────────────────────────────────

async fn register_agent_in_db(
    state: &AppState,
    agent_id: &str,
    tools: &[AgentToolInfo],
    sub_backends: &[AgentSubBackendInfo],
) -> Result<Uuid, String> {
    let config = serde_json::json!({
        "agent_id": agent_id,
        "sub_backends": sub_backends,
    });

    let existing: Option<(Uuid,)> =
        sqlx::query_as("SELECT backend_id FROM backends WHERE name = $1")
            .bind(agent_id)
            .fetch_optional(&state.db)
            .await
            .map_err(|e| format!("DB error: {}", e))?;

    let backend_id = if let Some((id,)) = existing {
        sqlx::query(
            "UPDATE backends SET transport = 'agent', config = $1, is_enabled = TRUE, \
             health_status = 'healthy', last_health_check = NOW() WHERE backend_id = $2",
        )
        .bind(&config)
        .bind(id)
        .execute(&state.db)
        .await
        .map_err(|e| format!("DB error: {}", e))?;
        id
    } else {
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO backends (backend_id, name, transport, config, risk_category, is_enabled, health_status, last_health_check) \
             VALUES ($1, $2, 'agent', $3, 'external-api', TRUE, 'healthy', NOW())",
        )
        .bind(id)
        .bind(agent_id)
        .bind(&config)
        .execute(&state.db)
        .await
        .map_err(|e| format!("DB error: {}", e))?;
        id
    };

    // Convert to DiscoveredTool and register using existing helper
    let discovered: Vec<crate::backends::DiscoveredTool> = tools
        .iter()
        .map(|t| crate::backends::DiscoveredTool {
            name: t.name.clone(),
            description: t.description.clone(),
            input_schema: t.input_schema.clone(),
        })
        .collect();

    crate::register_discovered_tools(&state.db, backend_id, agent_id, &discovered).await;

    tracing::info!(
        agent_id = %agent_id,
        backend_id = %backend_id,
        tool_count = tools.len(),
        "Agent tools registered in DB"
    );

    Ok(backend_id)
}
