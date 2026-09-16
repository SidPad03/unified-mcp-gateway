//! A local MCP server spoken to over streamable HTTP.
//!
//! There is no process to supervise here, so "running" means the last
//! `initialize` succeeded. A backend that goes away silently is noticed on the
//! next tool call, and the Backends page offers Restart to re-probe it.
//!
//! Three things every official SDK's server requires. This client used to do
//! none of them, so an HTTP server built on the Go, Python or TypeScript SDK
//! could not be added on a Mac at all:
//!
//! * **`Accept` naming both media types.** The Go SDK answers a request without
//!   it `400`, the Python SDK `406`.
//! * **Replies as an event stream.** A server may answer any POST with a one-shot
//!   `text/event-stream` body instead of JSON, and the SDKs mostly do.
//! * **Sessions.** A stateful server hands out `Mcp-Session-Id` in `initialize`
//!   and refuses anything without it — with `400`/`404` as the spec says, or, in
//!   the Go SDK's case, `200` and a JSON-RPC error. The gateway had the same
//!   blind spot for that last one (#13).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;

use crate::config::LocalBackendConfig;

pub const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(30);
pub const LIST_TOOLS_TIMEOUT: Duration = Duration::from_secs(30);
pub const CALL_TIMEOUT: Duration = Duration::from_secs(120);

const SESSION_HEADER: &str = "Mcp-Session-Id";

#[derive(Clone)]
pub struct HttpClient {
    url: String,
    // `reqwest::Client` is an Arc internally, so cloning this is cheap and the
    // connection pool is shared.
    client: reqwest::Client,
    /// The session the server opened, shared by every clone, so a session
    /// re-opened under one call is the one the next call sends.
    session: Arc<Mutex<Option<String>>>,
}

/// What a POST came back with.
enum Reply {
    /// A 2xx and the JSON-RPC message it carried.
    Message(Value),
    /// Anything else, and (the start of) its body.
    Refused(reqwest::StatusCode, String),
}

impl HttpClient {
    pub fn new(config: &LocalBackendConfig) -> Result<Self, String> {
        let url = config
            .url
            .as_deref()
            .filter(|u| !u.trim().is_empty())
            .ok_or_else(|| format!("Backend '{}' has no URL", config.name))?
            .to_string();

        // `Accept` first, so a header the user configured replaces it rather
        // than the other way round.
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::ACCEPT,
            reqwest::header::HeaderValue::from_static("application/json, text/event-stream"),
        );
        for (key, value) in &config.headers {
            let name = reqwest::header::HeaderName::from_bytes(key.as_bytes())
                .map_err(|_| format!("'{key}' is not a valid header name"))?;
            let value = reqwest::header::HeaderValue::from_str(value)
                .map_err(|_| format!("Header '{key}' has a value HTTP cannot carry"))?;
            headers.insert(name, value);
        }

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .map_err(|e| format!("Could not build the HTTP client: {e}"))?;

        Ok(Self {
            url,
            client,
            session: Arc::new(Mutex::new(None)),
        })
    }

    fn current_session(&self) -> Option<String> {
        self.session.lock().ok().and_then(|s| s.clone())
    }

    fn set_session(&self, session: Option<String>) {
        if let Ok(mut slot) = self.session.lock() {
            *slot = session;
        }
    }

    async fn post(
        &self,
        body: &Value,
        session: Option<&str>,
        timeout: Duration,
        what: &str,
    ) -> Result<(Reply, Option<String>), String> {
        let mut request = self.client.post(&self.url).json(body).timeout(timeout);
        if let Some(session) = session {
            request = request.header(SESSION_HEADER, session);
        }
        let response = request
            .send()
            .await
            .map_err(|e| format!("{what} request failed: {e}"))?;

        let status = response.status();
        let assigned = response
            .headers()
            .get(SESSION_HEADER)
            .and_then(|v| v.to_str().ok())
            .filter(|v| !v.is_empty())
            .map(str::to_string);
        let is_stream = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.to_ascii_lowercase().contains("text/event-stream"));
        let text = response
            .text()
            .await
            .map_err(|e| format!("Could not read the {what} response: {e}"))?;

        if !status.is_success() {
            let detail = text.chars().take(300).collect::<String>();
            return Ok((Reply::Refused(status, detail), assigned));
        }

        let message = if is_stream {
            parse_event_stream(&text).ok_or_else(|| {
                format!("{what} returned an event stream with no JSON-RPC message")
            })?
        } else {
            match serde_json::from_str(&text) {
                Ok(value) => value,
                // Unlabelled, but framed as events.
                Err(e) => parse_event_stream(&text)
                    .ok_or_else(|| format!("{what} did not return JSON: {e}"))?,
            }
        };
        Ok((Reply::Message(message), assigned))
    }

    /// One request, joining the current session; a refusal that means "open a
    /// session first" buys one handshake and one retry.
    async fn rpc(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
        let body = request_body(method, params);
        let sent = self.current_session();
        let (reply, _) = self.post(&body, sent.as_deref(), timeout, method).await?;

        let reply = if refused_for_want_of_session(&reply, sent.is_some()) {
            match self.handshake().await {
                Ok(Some(_)) => {
                    let session = self.current_session();
                    self.post(&body, session.as_deref(), timeout, method)
                        .await?
                        .0
                }
                // No session to be had, so the refusal was not about one: the
                // first answer, error and all, is the real one.
                Ok(None) => reply,
                Err(e) => return Err(e),
            }
        } else {
            reply
        };

        match reply {
            Reply::Refused(status, detail) => Err(format!("HTTP {status}: {detail}")),
            Reply::Message(parsed) => {
                if let Some(error) = parsed.get("error") {
                    let message = error
                        .get("message")
                        .and_then(|m| m.as_str())
                        .map(str::to_string)
                        .unwrap_or_else(|| error.to_string());
                    return Err(format!("JSON-RPC error: {message}"));
                }
                Ok(parsed.get("result").cloned().unwrap_or(parsed))
            }
        }
    }

    /// `initialize` + `notifications/initialized`. Keeps whatever session the
    /// server opened, and returns it.
    async fn handshake(&self) -> Result<Option<String>, String> {
        let body = request_body("initialize", super::initialize_params());
        let (reply, assigned) = self
            .post(&body, None, INITIALIZE_TIMEOUT, "initialize")
            .await?;
        match reply {
            Reply::Refused(status, detail) => return Err(format!("HTTP {status}: {detail}")),
            Reply::Message(parsed) => {
                if let Some(error) = parsed.get("error") {
                    return Err(format!("initialize failed: {error}"));
                }
            }
        }
        self.set_session(assigned.clone());

        // Best effort: a server that rejects the notification still works.
        let mut notify = self
            .client
            .post(&self.url)
            .json(&serde_json::json!({
                "jsonrpc": "2.0",
                "method": "notifications/initialized"
            }))
            .timeout(Duration::from_secs(10));
        if let Some(session) = &assigned {
            notify = notify.header(SESSION_HEADER, session);
        }
        let _ = notify.send().await;
        Ok(assigned)
    }

    pub async fn initialize(&self) -> Result<(), String> {
        self.handshake().await.map(|_| ())
    }

    pub async fn list_tools(&self) -> Result<Value, String> {
        self.rpc("tools/list", serde_json::json!({}), LIST_TOOLS_TIMEOUT)
            .await
    }

    pub async fn call_tool(&self, tool: &str, arguments: &Value) -> Result<Value, String> {
        self.rpc(
            "tools/call",
            serde_json::json!({ "name": tool, "arguments": arguments }),
            CALL_TIMEOUT,
        )
        .await
    }

    /// Tell the server this client is done with its session, if it has one.
    ///
    /// Best effort and bounded: the spec asks for the `DELETE`, and a server
    /// that ignores it expires the session on its own.
    pub async fn end_session(&self) {
        let Some(session) = self.session.lock().ok().and_then(|mut s| s.take()) else {
            return;
        };
        let _ = self
            .client
            .delete(&self.url)
            .header(SESSION_HEADER, session)
            .timeout(Duration::from_secs(5))
            .send()
            .await;
    }

    pub fn url(&self) -> &str {
        &self.url
    }
}

fn request_body(method: &str, params: Value) -> Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": uuid::Uuid::new_v4().to_string(),
        "method": method,
        "params": params,
    })
}

/// Whether a reply means the request needed a session.
///
/// Without a session, `400` and `404` are the spec's answers, and the Go SDK's
/// `200` with an error that says so in words counts too: a refused
/// session-less request cannot have run. With a session, `404` means it is
/// gone, and a `400` only counts when its body is about the session (the
/// TypeScript README server's way of saying so) — a `400` for anything else is
/// not cured by a new session. The words on a `200` never count inside a
/// session: that is more likely the tool's own error, and replaying a call
/// that may have run runs it twice. Kept in step with the gateway's
/// `BackendManager::refused_for_want_of_session`.
fn refused_for_want_of_session(reply: &Reply, sent_session: bool) -> bool {
    match reply {
        Reply::Refused(status, detail) => match *status {
            reqwest::StatusCode::NOT_FOUND => true,
            reqwest::StatusCode::BAD_REQUEST => !sent_session || names_a_missing_session(detail),
            _ => false,
        },
        Reply::Message(parsed) => {
            !sent_session
                && parsed
                    .get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(|m| m.as_str())
                    .is_some_and(names_a_missing_session)
        }
    }
}

/// The official SDKs' refusals, recognised by their wording — kept in step with
/// the gateway's `BackendManager::names_a_missing_session`.
fn names_a_missing_session(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    let about_a_session = message.contains("session")
        && [
            "initializ",
            "missing",
            "required",
            "not found",
            "no valid",
            "invalid session",
            "unknown session",
            "expired",
        ]
        .iter()
        .any(|phrase| message.contains(phrase));
    about_a_session || message.contains("not initialized")
}

/// The JSON-RPC reply in a one-shot `text/event-stream` body, preferring the
/// event that carries a `result` or `error` over a notification sent ahead of
/// it.
///
/// Every line ending the SSE spec allows is accepted. The Python SDK ends its
/// lines in CRLF, and splitting on `"\n\n"` never finds an event in that.
fn parse_event_stream(body: &str) -> Option<Value> {
    let normalized = body.replace("\r\n", "\n").replace('\r', "\n");
    let mut fallback = None;
    for event in normalized.split("\n\n") {
        let data = event
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .map(|value| value.strip_prefix(' ').unwrap_or(value))
            .collect::<Vec<_>>()
            .join("\n");
        if data.is_empty() {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<Value>(&data) {
            if value.get("result").is_some() || value.get("error").is_some() {
                return Some(value);
            }
            fallback.get_or_insert(value);
        }
    }
    fallback
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn rejects_a_backend_without_a_url() {
        let config = LocalBackendConfig {
            name: "x".into(),
            transport: "http".into(),
            ..Default::default()
        };
        assert!(HttpClient::new(&config).is_err());
    }

    #[test]
    fn rejects_a_header_name_http_cannot_carry() {
        let mut headers = std::collections::HashMap::new();
        headers.insert("bad header".to_string(), "value".to_string());
        let config = LocalBackendConfig {
            name: "x".into(),
            transport: "http".into(),
            url: Some("http://127.0.0.1:1/mcp".into()),
            headers,
            ..Default::default()
        };
        let err = HttpClient::new(&config).err().expect("should be rejected");
        assert!(err.contains("bad header"), "{err}");
    }

    #[tokio::test]
    async fn a_refused_connection_is_an_error_not_a_success() {
        // Port 1 on loopback: nothing listens, and connect() fails fast.
        let config = LocalBackendConfig {
            name: "x".into(),
            transport: "http".into(),
            url: Some("http://127.0.0.1:1/mcp".into()),
            ..Default::default()
        };
        let client = HttpClient::new(&config).unwrap();
        assert!(client.initialize().await.is_err());
    }

    #[test]
    fn event_stream_bodies_parse_with_any_line_ending() {
        for body in [
            "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"ok\":true}}\n\n",
            "event: message\r\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"ok\":true}}\r\n\r\n",
            "data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}\r\n\r\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"ok\":true}}\r\n\r\n",
        ] {
            let parsed = parse_event_stream(body).unwrap_or_else(|| panic!("{body:?}"));
            assert_eq!(parsed["result"]["ok"], true, "{body:?}");
        }
        assert!(parse_event_stream(": ping\r\n\r\n").is_none());
    }

    #[test]
    fn in_a_session_only_a_gone_session_triggers_a_handshake() {
        let refused = |status: u16, detail: &str| {
            Reply::Refused(
                reqwest::StatusCode::from_u16(status).unwrap(),
                detail.into(),
            )
        };
        assert!(refused_for_want_of_session(
            &refused(404, "session not found"),
            true
        ));
        assert!(refused_for_want_of_session(
            &refused(400, "Bad Request: No valid session ID provided"),
            true
        ));
        assert!(!refused_for_want_of_session(
            &refused(400, "duplicate in-flight request ID 3"),
            true
        ));
        assert!(refused_for_want_of_session(&refused(400, ""), false));
    }

    #[test]
    fn the_sdks_refusals_are_recognised() {
        for message in [
            "method \"tools/call\" is invalid during session initialization",
            "Bad Request: Missing session ID",
            "Session not found",
            "Bad Request: Server not initialized",
            "Bad Request: Mcp-Session-Id header is required",
        ] {
            assert!(names_a_missing_session(message), "{message}");
        }
        assert!(!names_a_missing_session("Invalid params"));
    }

    /// A server that behaves like go-sdk v1.8.0 in stateful mode, over a raw
    /// socket: it insists on `Accept`, refuses a session-less method with `200`
    /// and a code-0 error in an SSE body, and 404s a session it does not know.
    async fn go_sdk_server() -> (String, Arc<AtomicUsize>, Arc<AtomicUsize>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/mcp", listener.local_addr().unwrap());
        let initializes = Arc::new(AtomicUsize::new(0));
        let deletes = Arc::new(AtomicUsize::new(0));
        let sessions = Arc::new(Mutex::new(Vec::<String>::new()));
        let (inits, dels) = (initializes.clone(), deletes.clone());
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let (inits, dels, sessions) = (inits.clone(), dels.clone(), sessions.clone());
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 8192];
                    let head_end = loop {
                        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            break i;
                        }
                        match stream.read(&mut chunk).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        }
                    };
                    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
                    let method_line = head.lines().next().unwrap_or("").to_string();
                    let headers: HashMap<String, String> = head
                        .lines()
                        .skip(1)
                        .filter_map(|l| l.split_once(':'))
                        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
                        .collect();
                    let length: usize = headers
                        .get("content-length")
                        .and_then(|l| l.parse().ok())
                        .unwrap_or(0);
                    let mut body = buf[head_end + 4..].to_vec();
                    while body.len() < length {
                        match stream.read(&mut chunk).await {
                            Ok(0) | Err(_) => break,
                            Ok(n) => body.extend_from_slice(&chunk[..n]),
                        }
                    }
                    let json: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                    let session = headers.get("mcp-session-id").cloned();

                    let respond = |status: &str, extra: &str, body: String| {
                        format!(
                            "HTTP/1.1 {status}\r\nConnection: close\r\n{extra}Content-Length: {}\r\n\r\n{body}",
                            body.len()
                        )
                    };
                    let sse = |value: Value| format!("event: message\ndata: {value}\n\n");
                    let id = json["id"].clone();
                    let method = json["method"].as_str().unwrap_or("").to_string();

                    let out = if method_line.starts_with("DELETE") {
                        dels.fetch_add(1, Ordering::SeqCst);
                        respond("204 No Content", "", String::new())
                    } else if !headers.get("accept").is_some_and(|a| {
                        a.contains("application/json") && a.contains("text/event-stream")
                    }) {
                        respond(
                            "400 Bad Request",
                            "Content-Type: text/plain\r\n",
                            "Accept must contain both 'application/json' and 'text/event-stream'\n"
                                .into(),
                        )
                    } else if method == "initialize" {
                        let n = inits.fetch_add(1, Ordering::SeqCst);
                        let new = format!("S{n}");
                        sessions.lock().unwrap().push(new.clone());
                        respond(
                            "200 OK",
                            &format!(
                                "Content-Type: text/event-stream\r\nMcp-Session-Id: {new}\r\n"
                            ),
                            sse(serde_json::json!({"jsonrpc": "2.0", "id": id, "result": {}})),
                        )
                    } else if method.starts_with("notifications/") {
                        respond("202 Accepted", "", String::new())
                    } else if session.is_none() {
                        respond(
                            "200 OK",
                            "Content-Type: text/event-stream\r\n",
                            sse(serde_json::json!({"jsonrpc": "2.0", "id": id, "error": {
                                "code": 0,
                                "message": format!("method \"{method}\" is invalid during session initialization")
                            }})),
                        )
                    } else if session
                        .as_ref()
                        .is_some_and(|s| !sessions.lock().unwrap().contains(s))
                    {
                        respond(
                            "404 Not Found",
                            "Content-Type: text/plain\r\n",
                            "session not found\n".into(),
                        )
                    } else if method == "tools/list" {
                        respond(
                            "200 OK",
                            "Content-Type: text/event-stream\r\n",
                            sse(serde_json::json!({"jsonrpc": "2.0", "id": id, "result": {
                                "tools": [{"name": "echo", "inputSchema": {"type": "object"}}]
                            }})),
                        )
                    } else {
                        respond(
                            "200 OK",
                            "Content-Type: text/event-stream\r\n",
                            sse(serde_json::json!({"jsonrpc": "2.0", "id": id, "result": {
                                "content": [{"type": "text", "text": json["params"]["arguments"]["text"]}]
                            }})),
                        )
                    };
                    let _ = stream.write_all(out.as_bytes()).await;
                    let _ = stream.shutdown().await;
                });
            }
        });
        (url, initializes, deletes)
    }

    fn client_for(url: String) -> HttpClient {
        HttpClient::new(&LocalBackendConfig {
            name: "go".into(),
            transport: "http".into(),
            url: Some(url),
            ..Default::default()
        })
        .unwrap()
    }

    #[tokio::test]
    async fn a_stateful_go_sdk_server_works_end_to_end() {
        let (url, initializes, deletes) = go_sdk_server().await;
        let client = client_for(url);

        client.initialize().await.unwrap();
        let tools = client.list_tools().await.unwrap();
        assert_eq!(tools["tools"][0]["name"], "echo");
        for text in ["one", "two"] {
            let result = client
                .call_tool("echo", &serde_json::json!({ "text": text }))
                .await
                .unwrap();
            assert_eq!(result["content"][0]["text"], text);
        }
        assert_eq!(initializes.load(Ordering::SeqCst), 1, "one session, reused");

        client.end_session().await;
        assert_eq!(deletes.load(Ordering::SeqCst), 1);
    }

    /// The gateway can reach a backend long after it was initialized; a server
    /// restarted in between has forgotten the session and says 404.
    #[tokio::test]
    async fn a_forgotten_session_is_reopened() {
        let (url, initializes, _) = go_sdk_server().await;
        let client = client_for(url);
        client.initialize().await.unwrap();

        // Pretend the server restarted: present a session it never issued.
        client.set_session(Some("stale".into()));
        let result = client
            .call_tool("echo", &serde_json::json!({ "text": "back" }))
            .await
            .unwrap();
        assert_eq!(result["content"][0]["text"], "back");
        assert_eq!(initializes.load(Ordering::SeqCst), 2);
    }

    /// A clone made before the handshake, as the supervisor's call path makes
    /// them, still sends the session the handshake opened.
    #[tokio::test]
    async fn clones_share_the_session() {
        let (url, initializes, _) = go_sdk_server().await;
        let client = client_for(url);
        let clone = client.clone();
        client.initialize().await.unwrap();
        clone
            .call_tool("echo", &serde_json::json!({ "text": "x" }))
            .await
            .unwrap();
        assert_eq!(initializes.load(Ordering::SeqCst), 1);
    }
}
