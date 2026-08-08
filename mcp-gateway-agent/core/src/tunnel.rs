//! The WebSocket tunnel to the gateway.
//!
//! Connect, register the tools that are actually up, then sit in a read loop
//! forwarding tool calls to local backends and answers back. On any failure,
//! reconnect with exponential backoff.
//!
//! Two rules that predate this rewrite and still hold:
//!
//! * **Correlate by JSON-RPC id, never by name.** The gateway hands every call a
//!   `request_id`; it is the only safe way to match an answer to a question when
//!   two calls to the same tool are in flight.
//! * **Log argument counts, never arguments.** Tool arguments routinely contain
//!   credentials, file contents and personal data. The count is enough to debug
//!   a routing problem.

use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use crate::config::Config;
use crate::logbuf::LogLevel;
use crate::protocol::{AgentMessage, GatewayMessage};
use crate::state::{AgentState, ConnState};

/// How often an application-level ping goes out.
const PING_INTERVAL: Duration = Duration::from_secs(20);
/// No traffic at all for this long means the connection is dead, whatever TCP
/// thinks.
const PONG_TIMEOUT: Duration = Duration::from_secs(45);
const INITIAL_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// A connection has to last this long before it counts as stable and resets the
/// backoff. A gateway that accepts and immediately closes returns `Ok` too, and
/// resetting on that produces a reconnect once a second, forever.
const MIN_STABLE_UPTIME: Duration = Duration::from_secs(30);
/// How long to wait for the gateway's `registered` confirmation.
const REGISTER_TIMEOUT: Duration = Duration::from_secs(10);

/// Why a connection ended.
enum Outcome {
    /// The gateway or the network ended it; back off and retry.
    Closed(String),
    /// The app asked for a reconnect (settings changed, or the user pressed
    /// Reconnect); go again straight away.
    ReconnectRequested,
}

pub async fn run(state: Arc<AgentState>) {
    let mut backoff = INITIAL_BACKOFF;
    let mut attempt: u32 = 0;

    loop {
        let config = state.config().await;
        let api_key = state.api_key().await;

        // Nothing to connect to yet: the first-run wizard is on screen. Wait to
        // be told the settings changed rather than retrying an empty URL.
        if !config.is_configured() || api_key.is_empty() {
            state
                .update_connection(|c| {
                    c.state = ConnState::Idle;
                    c.backend_id = None;
                    c.connected_since = None;
                    c.retry_in_ms = None;
                    c.attempt = 0;
                })
                .await;
            state.reconnect.notified().await;
            attempt = 0;
            backoff = INITIAL_BACKOFF;
            continue;
        }

        attempt += 1;
        state
            .update_connection(|c| {
                c.state = if attempt == 1 {
                    ConnState::Connecting
                } else {
                    ConnState::Reconnecting
                };
                c.attempt = attempt;
                c.retry_in_ms = None;
                c.gateway_url = config.agent.gateway_url.clone();
                c.agent_id = config.agent.agent_id.clone();
            })
            .await;

        tracing::info!(
            gateway = %config.agent.gateway_url,
            agent_id = %config.agent.agent_id,
            attempt,
            "Connecting to the gateway"
        );

        let started = std::time::Instant::now();
        let outcome = connect_and_run(&state, &config, &api_key).await;

        state.set_writer(None).await;

        let stable = started.elapsed() >= MIN_STABLE_UPTIME;
        match outcome {
            Ok(Outcome::ReconnectRequested) => {
                backoff = INITIAL_BACKOFF;
                attempt = 0;
                continue;
            }
            Ok(Outcome::Closed(reason)) => {
                tracing::info!(%reason, "Connection closed; reconnecting");
                state
                    .update_connection(|c| {
                        c.state = ConnState::Reconnecting;
                        c.backend_id = None;
                        c.connected_since = None;
                        c.last_error = Some(reason);
                    })
                    .await;
                if stable {
                    backoff = INITIAL_BACKOFF;
                    attempt = 0;
                }
            }
            Err(error) => {
                tracing::error!(%error, retry_in_s = backoff.as_secs(), "Connection failed");
                state.logs.push(
                    LogLevel::Error,
                    "agent",
                    format!(
                        "Connection failed: {error}. Retrying in {}s",
                        backoff.as_secs()
                    ),
                );
                state
                    .update_connection(|c| {
                        c.state = ConnState::Reconnecting;
                        c.backend_id = None;
                        c.connected_since = None;
                        c.last_error = Some(error);
                    })
                    .await;
            }
        }

        state
            .update_connection(|c| c.retry_in_ms = Some(backoff.as_millis() as u64))
            .await;

        tokio::select! {
            _ = tokio::time::sleep(backoff) => {
                backoff = (backoff * 2).min(MAX_BACKOFF);
            }
            _ = state.reconnect.notified() => {
                backoff = INITIAL_BACKOFF;
                attempt = 0;
            }
        }
    }
}

async fn connect_and_run(
    state: &Arc<AgentState>,
    config: &Config,
    api_key: &str,
) -> Result<Outcome, String> {
    let request = authorized_request(&config.agent.gateway_url, api_key)?;
    let (ws, _response) = connect(request, config.agent.tls_skip_verify).await?;

    tracing::info!("WebSocket connected");
    let (mut sink, mut stream) = ws.split();

    // Every outgoing frame goes through this channel, so concurrent tool
    // responses and the ping timer cannot interleave halfway through a write.
    let (write_tx, mut write_rx) = mpsc::channel::<String>(64);
    let writer = tokio::spawn(async move {
        while let Some(text) = write_rx.recv().await {
            if sink.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
    });

    state.set_writer(Some(write_tx.clone())).await;

    if !state.send_register().await {
        writer.abort();
        return Err("Could not send the registration".into());
    }

    // Wait for the gateway to confirm. It also uses this window to reject a bad
    // credential — see `check_gateway`.
    let deadline = tokio::time::Instant::now() + REGISTER_TIMEOUT;
    let backend_id = loop {
        match tokio::time::timeout_at(deadline, stream.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => match serde_json::from_str(&text) {
                Ok(GatewayMessage::Registered { backend_id }) => break backend_id,
                Ok(GatewayMessage::Error { message }) => {
                    writer.abort();
                    return Err(format!("Gateway rejected the agent: {message}"));
                }
                _ => continue,
            },
            Ok(Some(Ok(_))) => continue,
            Ok(Some(Err(e))) => {
                writer.abort();
                return Err(format!("WebSocket error: {e}"));
            }
            Ok(None) => {
                writer.abort();
                return Err("Gateway closed the connection during registration".into());
            }
            Err(_) => {
                writer.abort();
                return Err("Timed out waiting for the gateway to confirm registration".into());
            }
        }
    };

    tracing::info!(%backend_id, "Registered with the gateway");
    state
        .update_connection(|c| {
            c.state = ConnState::Connected;
            c.backend_id = Some(backend_id);
            c.connected_since = Some(crate::now_rfc3339());
            c.last_error = None;
            c.retry_in_ms = None;
        })
        .await;

    let mut ping = tokio::time::interval(PING_INTERVAL);
    ping.tick().await; // the first tick is immediate; skip it
    let mut last_activity = tokio::time::Instant::now();

    let outcome = loop {
        tokio::select! {
            _ = state.reconnect.notified() => {
                break Outcome::ReconnectRequested;
            }

            _ = ping.tick() => {
                if last_activity.elapsed() > PONG_TIMEOUT {
                    break Outcome::Closed(format!(
                        "No response from the gateway in {}s",
                        PONG_TIMEOUT.as_secs()
                    ));
                }
                if write_tx.send(AgentMessage::Ping.to_frame()).await.is_err() {
                    break Outcome::Closed("Write channel closed".into());
                }
            }

            message = stream.next() => {
                match message {
                    Some(Ok(Message::Text(text))) => {
                        last_activity = tokio::time::Instant::now();
                        match serde_json::from_str::<GatewayMessage>(&text) {
                            Ok(GatewayMessage::ToolCall { request_id, tool, arguments }) => {
                                dispatch(state, &write_tx, request_id, tool, arguments).await;
                            }
                            Ok(GatewayMessage::Resync) => {
                                tracing::info!("Gateway asked for a resync");
                                state.send_register().await;
                            }
                            Ok(GatewayMessage::Error { message }) => {
                                tracing::error!(%message, "Gateway reported an error");
                                state.logs.push(
                                    LogLevel::Error,
                                    "agent",
                                    format!("Gateway error: {message}"),
                                );
                            }
                            Ok(GatewayMessage::Pong) | Ok(GatewayMessage::Registered { .. }) => {}
                            Err(e) => {
                                tracing::debug!(error = %e, "Ignoring an unparseable gateway message");
                            }
                        }
                    }
                    // tungstenite queues the protocol-level Pong itself and the
                    // writer task flushes it on its next send, so there is
                    // nothing to do here but note that the gateway is alive.
                    Some(Ok(Message::Ping(_))) => {
                        last_activity = tokio::time::Instant::now();
                    }
                    Some(Ok(Message::Close(_))) => {
                        break Outcome::Closed("Gateway closed the connection".into());
                    }
                    Some(Ok(_)) => {
                        last_activity = tokio::time::Instant::now();
                    }
                    Some(Err(e)) => break Outcome::Closed(format!("WebSocket error: {e}")),
                    None => break Outcome::Closed("Stream ended".into()),
                }
            }
        }
    };

    writer.abort();
    Ok(outcome)
}

/// Run one tool call to completion, off the read loop.
///
/// Spawned rather than awaited: a 120-second tool call must not stop the agent
/// answering pings or accepting other calls.
async fn dispatch(
    state: &Arc<AgentState>,
    write_tx: &mpsc::Sender<String>,
    request_id: String,
    tool: String,
    arguments: serde_json::Value,
) {
    let backend = state.backends.route_of(&tool).await;
    state.calls.start(&request_id, &tool, backend.clone());

    tracing::info!(
        %request_id,
        %tool,
        backend = backend.as_deref().unwrap_or("unrouted"),
        // Counts, never values.
        arg_count = arguments.as_object().map(|o| o.len()).unwrap_or(0),
        "Tool call received"
    );

    let state = state.clone();
    let write_tx = write_tx.clone();
    tokio::spawn(async move {
        let started = std::time::Instant::now();
        let result = state.backends.call_tool(&tool, &arguments).await;
        let duration_ms = started.elapsed().as_millis() as u64;

        let frame = match &result {
            Ok(value) => AgentMessage::ToolResult {
                request_id: request_id.clone(),
                result: value.clone(),
            },
            Err(error) => AgentMessage::ToolError {
                request_id: request_id.clone(),
                error: error.clone(),
            },
        };
        let _ = write_tx.send(frame.to_frame()).await;

        state.calls.complete(&request_id, duration_ms, result.err());
        tracing::info!(%request_id, duration_ms, "Tool call completed");
    });
}

// ── Connecting ──────────────────────────────────────────────────────────

type WsStream =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// The upgrade request, with the API key as an `Authorization` header.
///
/// It used to travel as `?token=` in the URL, and a URL is the one part of a
/// request that everything logs: the gateway's own `tower_http` traces, any
/// reverse proxy's access log. The credential was being written to disk on
/// every connect and every retry. The gateway has always accepted the header
/// form as well (`agent/mod.rs` falls back to `Authorization` when `?token=`
/// is absent), so nothing older breaks.
fn authorized_request(
    url: &str,
    api_key: &str,
) -> Result<tokio_tungstenite::tungstenite::handshake::client::Request, String> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    let mut request = url
        .into_client_request()
        .map_err(|e| describe_connect_error(&e))?;
    let value = format!("Bearer {api_key}")
        .parse()
        .map_err(|_| "The API key contains characters that cannot travel in a header".to_string())?;
    request.headers_mut().insert("authorization", value);
    Ok(request)
}

async fn connect(
    request: tokio_tungstenite::tungstenite::handshake::client::Request,
    tls_skip_verify: bool,
) -> Result<
    (
        WsStream,
        tokio_tungstenite::tungstenite::handshake::client::Response,
    ),
    String,
> {
    let attempt = async {
        if tls_skip_verify {
            let tls = rustls::ClientConfig::builder()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(NoVerifier))
                .with_no_client_auth();
            let connector = tokio_tungstenite::Connector::Rustls(Arc::new(tls));
            tokio_tungstenite::connect_async_tls_with_config(request, None, false, Some(connector))
                .await
        } else {
            tokio_tungstenite::connect_async(request).await
        }
    };

    match tokio::time::timeout(CONNECT_TIMEOUT, attempt).await {
        Ok(Ok(pair)) => Ok(pair),
        Ok(Err(e)) => Err(describe_connect_error(&e)),
        Err(_) => Err(format!(
            "Timed out after {}s connecting to the gateway",
            CONNECT_TIMEOUT.as_secs()
        )),
    }
}

fn describe_connect_error(error: &tokio_tungstenite::tungstenite::Error) -> String {
    use tokio_tungstenite::tungstenite::Error;
    match error {
        Error::Http(response) => format!(
            "Gateway returned HTTP {} to the WebSocket upgrade",
            response.status()
        ),
        Error::Io(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
            "Connection refused — is the gateway running at that address?".to_string()
        }
        Error::Url(e) => format!("That is not a usable WebSocket URL: {e}"),
        // What tungstenite returns when the string does not parse as a URI at
        // all — "gw.example.com" without a scheme, say. Its own wording
        // ("HTTP format error: invalid format") tells the user nothing.
        Error::HttpFormat(_) => "That is not a usable WebSocket URL — it should look like \
             wss://host/agent/ws"
            .to_string(),
        Error::Tls(e) => format!(
            "TLS handshake failed: {e}. If the gateway uses a self-signed \
             certificate, turn on 'Skip TLS verification'."
        ),
        other => format!("{other}"),
    }
}

// ── Wizard check ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct GatewayCheck {
    pub reachable: bool,
    pub authenticated: bool,
    pub detail: String,
}

/// Probe a gateway URL and credential without registering anything.
///
/// Defect #8: the old wizard did `timeout(..).await.is_ok()`, which is true
/// whenever the connect *returned* — including connection-refused. It reported a
/// dead gateway as valid and the user found out later, from a agent that would
/// not start.
///
/// The check also has to read the first frame, not just the handshake. The
/// gateway upgrades the WebSocket *before* it looks at the token and rejects a
/// bad one with an `error` message on the open socket, so a successful
/// handshake on its own proves reachability and nothing about the credential.
pub async fn check_gateway(
    gateway_url: &str,
    api_key: &str,
    tls_skip_verify: bool,
) -> GatewayCheck {
    let request = match authorized_request(gateway_url, api_key) {
        Ok(request) => request,
        Err(detail) => {
            return GatewayCheck {
                reachable: false,
                authenticated: false,
                detail,
            }
        }
    };

    let (ws, _) = match connect(request, tls_skip_verify).await {
        Ok(pair) => pair,
        Err(detail) => {
            return GatewayCheck {
                reachable: false,
                authenticated: false,
                detail,
            }
        }
    };

    let (mut sink, mut stream) = ws.split();

    // The gateway says nothing to a valid agent until it registers, so silence
    // is the success signal.
    let verdict = match tokio::time::timeout(Duration::from_secs(3), stream.next()).await {
        Ok(Some(Ok(Message::Text(text)))) => match serde_json::from_str::<GatewayMessage>(&text) {
            Ok(GatewayMessage::Error { message }) => GatewayCheck {
                reachable: true,
                authenticated: false,
                detail: message,
            },
            _ => GatewayCheck {
                reachable: true,
                authenticated: true,
                detail: "Gateway reachable and the API key was accepted".into(),
            },
        },
        Ok(Some(Ok(Message::Close(frame)))) => GatewayCheck {
            reachable: true,
            authenticated: false,
            detail: frame
                .map(|f| f.reason.to_string())
                .filter(|r| !r.is_empty())
                .unwrap_or_else(|| "Gateway closed the connection immediately".into()),
        },
        Ok(Some(Err(e))) => GatewayCheck {
            reachable: true,
            authenticated: false,
            detail: format!("WebSocket error: {e}"),
        },
        Ok(None) => GatewayCheck {
            reachable: true,
            authenticated: false,
            detail: "Gateway closed the connection immediately".into(),
        },
        // Silence for three seconds: registered agents are not spoken to until
        // they speak first.
        Err(_) | Ok(Some(Ok(_))) => GatewayCheck {
            reachable: true,
            authenticated: true,
            detail: "Gateway reachable and the API key was accepted".into(),
        },
    };

    let _ = sink.close().await;
    verdict
}

/// Accepts any server certificate. Only ever used when the user has explicitly
/// turned on "Skip TLS verification" for a self-signed gateway.
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
        use rustls::SignatureScheme::*;
        vec![
            RSA_PKCS1_SHA256,
            RSA_PKCS1_SHA384,
            RSA_PKCS1_SHA512,
            ECDSA_NISTP256_SHA256,
            ECDSA_NISTP384_SHA384,
            ECDSA_NISTP521_SHA512,
            RSA_PSS_SHA256,
            RSA_PSS_SHA384,
            RSA_PSS_SHA512,
            ED25519,
            ED448,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_token_is_appended_as_a_query_parameter() {
        assert_eq!(
            with_token("wss://gw/agent/ws", "k"),
            "wss://gw/agent/ws?token=k"
        );
        // A URL that already carries a query keeps it.
        assert_eq!(
            with_token("wss://gw/connect?v=2", "k"),
            "wss://gw/connect?v=2&token=k"
        );
    }

    #[tokio::test]
    async fn a_dead_gateway_is_reported_as_unreachable() {
        // Defect #8: this is the case the old `is_ok()` check called valid.
        // Port 1 on loopback refuses immediately.
        let check = check_gateway("ws://127.0.0.1:1/agent/ws", "key", false).await;
        assert!(!check.reachable, "{check:?}");
        assert!(!check.authenticated);
        assert!(!check.detail.is_empty());
    }

    #[tokio::test]
    async fn a_url_that_is_not_a_websocket_url_is_reported_clearly() {
        let check = check_gateway("not-a-url", "key", false).await;
        assert!(!check.reachable);
        assert!(
            check.detail.contains("URL") || check.detail.contains("url"),
            "{}",
            check.detail
        );
    }

    #[tokio::test]
    async fn a_gateway_that_rejects_the_key_is_reachable_but_unauthenticated() {
        use tokio::net::TcpListener;

        // A one-shot server that completes the WebSocket handshake and then
        // does exactly what the real gateway does with a bad token: sends an
        // `error` frame on the open socket.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(socket).await.unwrap();
            let _ = ws
                .send(Message::Text(
                    r#"{"type":"error","message":"Invalid API key"}"#.into(),
                ))
                .await;
            let _ = ws.close(None).await;
        });

        let check = check_gateway(&format!("ws://127.0.0.1:{port}/agent/ws"), "wrong", false).await;
        assert!(check.reachable, "{check:?}");
        assert!(!check.authenticated, "{check:?}");
        assert_eq!(check.detail, "Invalid API key");
    }

    #[tokio::test]
    async fn a_gateway_that_stays_quiet_means_the_key_was_accepted() {
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(socket).await.unwrap();
            // The real gateway waits up to 30s for a `register` and says
            // nothing meanwhile.
            tokio::time::sleep(Duration::from_secs(10)).await;
            let _ = ws.close(None).await;
        });

        let check = check_gateway(&format!("ws://127.0.0.1:{port}/agent/ws"), "good", false).await;
        assert!(check.reachable && check.authenticated, "{check:?}");
    }
}
