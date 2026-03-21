use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use crate::config::Config;
use crate::local_backends::{LocalBackendManager, SubBackendInfo};
use crate::tui::events::{AgentEvent, ConnectionState, LogLevel};

// ── Wire protocol (must match mcp-gateway-server/src/agent/mod.rs) ─────────────

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AgentMessage {
    #[serde(rename = "register")]
    Register {
        agent_id: String,
        tools: Vec<AgentToolInfo>,
        #[serde(skip_serializing_if = "Vec::is_empty", default)]
        backends: Vec<SubBackendInfo>,
    },
    #[serde(rename = "tool_result")]
    ToolResult { request_id: String, result: Value },
    #[serde(rename = "tool_error")]
    ToolError { request_id: String, error: String },
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

// ── Tunnel timings ──────────────────────────────────────────────────────

/// How often we send an application-level ping.
const PING_INTERVAL: Duration = Duration::from_secs(20);
/// If we haven't received ANY message (pong, tool call, etc.) within this window,
/// consider the connection dead and force a reconnect.
const PONG_TIMEOUT: Duration = Duration::from_secs(45);
/// Maximum reconnect backoff.
const MAX_BACKOFF: Duration = Duration::from_secs(30);
/// WebSocket connect timeout.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

// ── Tunnel with auto-reconnect ──────────────────────────────────────────

fn emit(events: &Option<mpsc::UnboundedSender<AgentEvent>>, event: AgentEvent) {
    if let Some(tx) = events {
        let _ = tx.send(event);
    }
}

pub async fn run_tunnel(
    config: &Config,
    manager: Arc<LocalBackendManager>,
    events: Option<mpsc::UnboundedSender<AgentEvent>>,
) -> ! {
    let mut delay = Duration::from_secs(1);
    let mut attempt: u32 = 0;

    loop {
        attempt += 1;
        tracing::info!(
            gateway = %config.agent.gateway_url,
            agent_id = %config.agent.agent_id,
            attempt,
            "Connecting to gateway..."
        );

        emit(&events, AgentEvent::ConnectionStatus(ConnectionState::Connecting));
        emit(&events, AgentEvent::Log {
            level: LogLevel::Info,
            message: format!("Connecting to {} (attempt {})...", config.agent.gateway_url, attempt),
        });

        match connect_and_run(config, &manager, &events).await {
            Ok(()) => {
                tracing::info!("Connection closed cleanly, reconnecting...");
                emit(&events, AgentEvent::Log {
                    level: LogLevel::Info,
                    message: "Connection closed, reconnecting...".to_string(),
                });
                delay = Duration::from_secs(1);
                attempt = 0;
            }
            Err(e) => {
                tracing::error!(
                    error = %e,
                    retry_in = ?delay,
                    "Connection failed, will retry"
                );
                emit(&events, AgentEvent::ConnectionStatus(
                    ConnectionState::Reconnecting(attempt),
                ));
                emit(&events, AgentEvent::Log {
                    level: LogLevel::Error,
                    message: format!("Connection failed: {}. Retrying in {}s", e, delay.as_secs()),
                });
            }
        }

        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(MAX_BACKOFF);
    }
}

async fn connect_and_run(
    config: &Config,
    manager: &Arc<LocalBackendManager>,
    events: &Option<mpsc::UnboundedSender<AgentEvent>>,
) -> anyhow::Result<()> {
    // Connect with token in query param
    let url = format!(
        "{}?token={}",
        config.agent.gateway_url, config.agent.api_key
    );

    let ws_connect = async {
        if config.agent.tls_skip_verify {
            let tls_config = rustls::ClientConfig::builder()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(NoVerifier))
                .with_no_client_auth();
            let connector = tokio_tungstenite::Connector::Rustls(Arc::new(tls_config));
            tokio_tungstenite::connect_async_tls_with_config(
                &url,
                None,
                false,
                Some(connector),
            ).await
        } else {
            tokio_tungstenite::connect_async(&url).await
        }
    };

    let (ws_stream, _response) = tokio::time::timeout(CONNECT_TIMEOUT, ws_connect)
        .await
        .map_err(|_| anyhow::anyhow!("Connection timed out after {:?}", CONNECT_TIMEOUT))?
        .map_err(|e| anyhow::anyhow!("WebSocket connect failed: {}", e))?;

    tracing::info!("WebSocket connected to gateway");
    emit(events, AgentEvent::Log {
        level: LogLevel::Info,
        message: "WebSocket connected to gateway".to_string(),
    });

    let (mut write, mut read) = ws_stream.split();

    // Send register message with all discovered tools
    let tools: Vec<AgentToolInfo> = manager
        .all_tools()
        .iter()
        .map(|t| AgentToolInfo {
            name: t.name.clone(),
            description: t.description.clone(),
            input_schema: t.input_schema.clone(),
        })
        .collect();

    let sub_backends = manager.sub_backends();
    let register = AgentMessage::Register {
        agent_id: config.agent.agent_id.clone(),
        tools: tools.clone(),
        backends: sub_backends,
    };
    let register_json = serde_json::to_string(&register)?;
    write.send(Message::Text(register_json)).await?;

    tracing::info!(tool_count = tools.len(), "Sent register message");
    emit(events, AgentEvent::Log {
        level: LogLevel::Info,
        message: format!("Registered {} tools with gateway", tools.len()),
    });

    // Wait for registered confirmation (10s timeout)
    let confirm_deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let backend_id = loop {
        match tokio::time::timeout_at(confirm_deadline, read.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => {
                match serde_json::from_str::<GatewayMessage>(&text) {
                    Ok(GatewayMessage::Registered { backend_id }) => break backend_id,
                    Ok(GatewayMessage::Error { message }) => {
                        return Err(anyhow::anyhow!("Gateway rejected registration: {}", message));
                    }
                    _ => continue,
                }
            }
            Ok(Some(Ok(_))) => continue,
            Ok(Some(Err(e))) => return Err(anyhow::anyhow!("WS error: {}", e)),
            Ok(None) => return Err(anyhow::anyhow!("Connection closed before registration")),
            Err(_) => return Err(anyhow::anyhow!("Timeout waiting for registration confirmation")),
        }
    };

    tracing::info!(backend_id = %backend_id, "Registered with gateway");
    emit(events, AgentEvent::Registered {
        backend_id: backend_id.clone(),
    });
    emit(events, AgentEvent::ConnectionStatus(ConnectionState::Connected));

    // Set up write channel: tool execution tasks send responses through this
    let (write_tx, mut write_rx) = mpsc::channel::<Message>(64);

    // Spawn the writer task
    let write_task = tokio::spawn(async move {
        while let Some(msg) = write_rx.recv().await {
            if write.send(msg).await.is_err() {
                break;
            }
        }
    });

    // Main read loop with ping/pong timeout for liveness detection
    let mut ping_interval = tokio::time::interval(PING_INTERVAL);
    ping_interval.tick().await; // skip first immediate tick
    let mut last_activity = tokio::time::Instant::now();

    loop {
        tokio::select! {
            // Periodic ping
            _ = ping_interval.tick() => {
                // Check if we've heard anything from the server recently
                if last_activity.elapsed() > PONG_TIMEOUT {
                    tracing::warn!("No response from gateway in {:?}, connection presumed dead", PONG_TIMEOUT);
                    emit(events, AgentEvent::ConnectionStatus(
                        ConnectionState::Disconnected("Ping timeout — no response from gateway".to_string()),
                    ));
                    break;
                }

                let ping = AgentMessage::Ping;
                let text = serde_json::to_string(&ping).unwrap();
                if write_tx.send(Message::Text(text)).await.is_err() {
                    break;
                }
            }

            // Incoming messages
            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        last_activity = tokio::time::Instant::now();

                        match serde_json::from_str::<GatewayMessage>(&text) {
                            Ok(GatewayMessage::ToolCall {
                                request_id,
                                tool,
                                arguments,
                            }) => {
                                tracing::info!(request_id = %request_id, tool = %tool, "Received tool call");

                                emit(events, AgentEvent::ToolCallReceived {
                                    request_id: request_id.clone(),
                                    tool: tool.clone(),
                                });

                                let mgr = manager.clone();
                                let tx = write_tx.clone();
                                let events_tx = events.clone();
                                let tool_clone = tool.clone();
                                let req_id = request_id.clone();
                                tokio::spawn(async move {
                                    let start = std::time::Instant::now();
                                    let result = mgr.call_tool(&tool_clone, &arguments).await;
                                    let duration_ms = start.elapsed().as_millis() as u64;
                                    let success = result.is_ok();

                                    let response = match result {
                                        Ok(val) => AgentMessage::ToolResult {
                                            request_id: req_id.clone(),
                                            result: val,
                                        },
                                        Err(err) => AgentMessage::ToolError {
                                            request_id: req_id.clone(),
                                            error: err,
                                        },
                                    };
                                    let text = serde_json::to_string(&response).unwrap();
                                    let _ = tx.send(Message::Text(text)).await;

                                    emit(&events_tx, AgentEvent::ToolCallCompleted {
                                        request_id: req_id,
                                        tool: tool_clone,
                                        duration_ms,
                                        success,
                                    });

                                    tracing::info!(request_id = %request_id, "Tool call completed");
                                });
                            }
                            Ok(GatewayMessage::Pong) => {}
                            Ok(GatewayMessage::Registered { .. }) => {}
                            Ok(GatewayMessage::Resync) => {
                                tracing::info!("Gateway requested resync, re-sending tool registration");
                                emit(events, AgentEvent::Log {
                                    level: LogLevel::Info,
                                    message: "Gateway requested resync, re-registering tools...".to_string(),
                                });

                                let resync_tools: Vec<AgentToolInfo> = manager
                                    .all_tools()
                                    .iter()
                                    .map(|t| AgentToolInfo {
                                        name: t.name.clone(),
                                        description: t.description.clone(),
                                        input_schema: t.input_schema.clone(),
                                    })
                                    .collect();
                                let sub_backends = manager.sub_backends();
                                let register = AgentMessage::Register {
                                    agent_id: config.agent.agent_id.clone(),
                                    tools: resync_tools,
                                    backends: sub_backends,
                                };
                                let text = serde_json::to_string(&register).unwrap();
                                if write_tx.send(Message::Text(text)).await.is_err() {
                                    break;
                                }
                                emit(events, AgentEvent::Log {
                                    level: LogLevel::Info,
                                    message: "Resync complete — tools re-registered".to_string(),
                                });
                            }
                            Ok(GatewayMessage::Error { message }) => {
                                tracing::error!(error = %message, "Gateway sent error");
                                emit(events, AgentEvent::Log {
                                    level: LogLevel::Error,
                                    message: format!("Gateway error: {}", message),
                                });
                            }
                            Err(e) => {
                                tracing::debug!(error = %e, "Ignoring unparseable gateway message");
                            }
                        }
                    }
                    Some(Ok(Message::Ping(data))) => {
                        last_activity = tokio::time::Instant::now();
                        let _ = write_tx.send(Message::Pong(data)).await;
                    }
                    Some(Ok(Message::Pong(_))) => {
                        last_activity = tokio::time::Instant::now();
                    }
                    Some(Ok(Message::Close(_))) => {
                        tracing::info!("Gateway closed connection");
                        emit(events, AgentEvent::ConnectionStatus(
                            ConnectionState::Disconnected("Gateway closed connection".to_string()),
                        ));
                        break;
                    }
                    Some(Ok(_)) => {
                        last_activity = tokio::time::Instant::now();
                    }
                    Some(Err(e)) => {
                        tracing::warn!(error = %e, "WebSocket error");
                        emit(events, AgentEvent::ConnectionStatus(
                            ConnectionState::Disconnected(e.to_string()),
                        ));
                        break;
                    }
                    None => {
                        tracing::info!("WebSocket stream ended");
                        emit(events, AgentEvent::ConnectionStatus(
                            ConnectionState::Disconnected("Stream ended".to_string()),
                        ));
                        break;
                    }
                }
            }
        }
    }

    // Cleanup
    write_task.abort();

    Ok(())
}

/// TLS certificate verifier that accepts any certificate (for self-signed certs)
#[derive(Debug)]
struct NoVerifier;

impl rustls::client::danger::ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls_pki_types::CertificateDer<'_>,
        _intermediates: &[rustls_pki_types::CertificateDer<'_>],
        _server_name: &rustls_pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls_pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls_pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls_pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::ECDSA_NISTP521_SHA512,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
            rustls::SignatureScheme::ED25519,
            rustls::SignatureScheme::ED448,
        ]
    }
}
