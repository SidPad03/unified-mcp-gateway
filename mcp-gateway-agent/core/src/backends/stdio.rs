//! A local MCP server spoken to over stdin/stdout.
//!
//! The process handle deliberately does **not** live in here. `spawn` hands the
//! `Child` back to the supervisor, which owns it and `wait()`s on it; this
//! module keeps only the pipes, behind a `Mutex<Option<..>>` that is `None`
//! while the backend is down. The old agent stored the child as `_child` and
//! never awaited it, so a backend that died stayed "running" forever — defect #3
//! in the design doc.

use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;

use crate::config::LocalBackendConfig;

pub const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(30);
pub const LIST_TOOLS_TIMEOUT: Duration = Duration::from_secs(30);
pub const CALL_TIMEOUT: Duration = Duration::from_secs(120);

/// The live pipes of a running backend.
pub struct StdioSession {
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

/// What `spawn` produces: pipes for us, the child and its stderr for the
/// supervisor.
pub struct Spawned {
    pub session: StdioSession,
    pub child: Child,
    pub stderr: ChildStderr,
    pub pid: Option<u32>,
}

/// A handle that outlives any single process.
///
/// The supervisor swaps sessions in and out on restart; callers hold this and
/// never notice, beyond getting a clear error while the slot is empty.
#[derive(Clone)]
pub struct StdioClient {
    session: Arc<Mutex<Option<StdioSession>>>,
}

impl Default for StdioClient {
    fn default() -> Self {
        Self::new()
    }
}

impl StdioClient {
    pub fn new() -> Self {
        Self {
            session: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn install(&self, session: StdioSession) {
        *self.session.lock().await = Some(session);
    }

    pub async fn clear(&self) {
        *self.session.lock().await = None;
    }

    pub async fn request(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, String> {
        let mut guard = self.session.lock().await;
        let session = guard
            .as_mut()
            .ok_or_else(|| "Backend is not running".to_string())?;
        session.request(method, params, timeout).await
    }

    pub async fn notify(&self, method: &str, params: Value) -> Result<(), String> {
        let mut guard = self.session.lock().await;
        let session = guard
            .as_mut()
            .ok_or_else(|| "Backend is not running".to_string())?;
        session.notify(method, params).await
    }

    pub async fn call_tool(&self, tool: &str, arguments: &Value) -> Result<Value, String> {
        self.request(
            "tools/call",
            serde_json::json!({ "name": tool, "arguments": arguments }),
            CALL_TIMEOUT,
        )
        .await
    }
}

impl StdioSession {
    /// One JSON-RPC round trip.
    ///
    /// Responses are matched **by id**. Skipping anything else is not
    /// pedantry: MCP servers send notifications (no id) and server-initiated
    /// *requests* (`sampling/createMessage`, `roots/list`) that do carry an id,
    /// and treating one of those as our response returns garbage and desyncs the
    /// pipe for every later call.
    pub async fn request(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, String> {
        let id = uuid::Uuid::new_v4().to_string();
        let mut request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
        });
        if !params.is_null() {
            request["params"] = params;
        }

        let mut line = serde_json::to_string(&request).map_err(|e| format!("Serialize: {e}"))?;
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| format!("Write to stdin: {e}"))?;
        self.stdin
            .flush()
            .await
            .map_err(|e| format!("Flush stdin: {e}"))?;

        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let mut response = String::new();
            match tokio::time::timeout_at(deadline, self.stdout.read_line(&mut response)).await {
                Ok(Ok(0)) => return Err("Backend closed stdout (EOF)".into()),
                Ok(Ok(_)) => {
                    let trimmed = response.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    let Ok(parsed) = serde_json::from_str::<Value>(trimmed) else {
                        // Some servers print banners to stdout before speaking
                        // JSON-RPC. Not ours to fix; skip and keep reading.
                        continue;
                    };
                    match parsed.get("id") {
                        Some(Value::String(resp_id)) if resp_id == &id => {}
                        _ => continue,
                    }
                    if let Some(error) = parsed.get("error") {
                        let message = error
                            .get("message")
                            .and_then(|m| m.as_str())
                            .unwrap_or("Unknown error");
                        return Err(format!("JSON-RPC error: {message}"));
                    }
                    return Ok(parsed.get("result").cloned().unwrap_or(Value::Null));
                }
                Ok(Err(e)) => return Err(format!("Read from stdout: {e}")),
                Err(_) => {
                    return Err(format!(
                        "Timed out after {}s waiting for '{method}'",
                        timeout.as_secs()
                    ))
                }
            }
        }
    }

    pub async fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        let mut request = serde_json::json!({ "jsonrpc": "2.0", "method": method });
        if !params.is_null() {
            request["params"] = params;
        }
        let mut line = serde_json::to_string(&request).map_err(|e| format!("Serialize: {e}"))?;
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| format!("Write: {e}"))?;
        self.stdin.flush().await.map_err(|e| format!("Flush: {e}"))
    }
}

/// Start the backend process.
pub fn spawn(config: &LocalBackendConfig) -> Result<Spawned, String> {
    let command = config
        .command
        .as_deref()
        .filter(|c| !c.trim().is_empty())
        .ok_or_else(|| format!("Backend '{}' has no command", config.name))?;

    let mut cmd = Command::new(command);
    cmd.args(&config.args)
        .env("PATH", super::login_path())
        .envs(&config.env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        // Piped, not inherited. In an app there is no terminal to inherit, so
        // the old `Stdio::inherit()` sent every backend's diagnostics to
        // /dev/null — defect #4. It feeds the Logs page now.
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to start '{command}': {e}"))?;

    let pid = child.id();
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| "Could not capture stdin".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Could not capture stdout".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "Could not capture stderr".to_string())?;

    Ok(Spawned {
        session: StdioSession {
            stdin: BufWriter::new(stdin),
            stdout: BufReader::new(stdout),
        },
        child,
        stderr,
        pid,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LocalBackendConfig;

    fn echo_server(script: &str) -> LocalBackendConfig {
        LocalBackendConfig {
            name: "probe".into(),
            transport: "stdio".into(),
            command: Some("/bin/sh".into()),
            args: vec!["-c".into(), script.into()],
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn a_server_initiated_request_is_not_mistaken_for_our_response() {
        // The backend answers our `initialize` only *after* sending a
        // notification and a request of its own — both of which the old
        // "first line wins" reader would have returned as the result.
        let script = r#"
            read -r line
            printf '%s\n' '{"jsonrpc":"2.0","method":"notifications/message","params":{}}'
            printf '%s\n' '{"jsonrpc":"2.0","id":"server-1","method":"roots/list"}'
            id=$(printf '%s' "$line" | sed 's/.*"id":"\([^"]*\)".*/\1/')
            printf '{"jsonrpc":"2.0","id":"%s","result":{"ok":true}}\n' "$id"
            sleep 5
        "#;
        let mut spawned = spawn(&echo_server(script)).unwrap();
        let result = spawned
            .session
            .request("initialize", serde_json::json!({}), Duration::from_secs(10))
            .await
            .unwrap();
        assert_eq!(result, serde_json::json!({"ok": true}));
        let _ = spawned.child.kill().await;
    }

    #[tokio::test]
    async fn a_backend_that_exits_reports_eof_not_a_hang() {
        let mut spawned = spawn(&echo_server("exit 3")).unwrap();

        // Reap the child before asking it anything. Without this the test races
        // the kernel: `stdin` is a `BufWriter`, so a request this small never
        // reaches a syscall in `write_all` — it is the *flush* that touches the
        // pipe. If the exit has not been observed yet the flush succeeds into a
        // pipe nobody will read and the failure arrives later as EOF; if it has,
        // the flush takes EPIPE. Both are correct, and which one you get is not
        // this test's business — but an assertion listing error spellings
        // silently depended on it, passed on macOS, and flaked on the Linux
        // runner.
        let _ = spawned.child.wait().await;

        let err = spawned
            .session
            .request("initialize", serde_json::json!({}), Duration::from_secs(5))
            .await
            .unwrap_err();

        // The contract is in the name: fail promptly rather than leaving the
        // request outstanding until the timeout. Assert that, not the wording.
        assert!(
            !err.contains("Timed out"),
            "a backend that exits must fail fast, not hang: {err}"
        );
    }

    #[tokio::test]
    async fn a_missing_command_fails_immediately_with_the_name_in_the_error() {
        let err = spawn(&LocalBackendConfig {
            name: "nope".into(),
            transport: "stdio".into(),
            command: Some("definitely-not-a-real-binary-xyz".into()),
            ..Default::default()
        })
        .err()
        .expect("a missing binary must not spawn");
        assert!(err.contains("definitely-not-a-real-binary-xyz"), "{err}");
    }

    #[tokio::test]
    async fn a_slow_backend_times_out_rather_than_blocking_forever() {
        let mut spawned = spawn(&echo_server("read -r _; sleep 30")).unwrap();
        let err = spawned
            .session
            .request(
                "initialize",
                serde_json::json!({}),
                Duration::from_millis(300),
            )
            .await
            .unwrap_err();
        assert!(err.contains("Timed out"), "{err}");
        let _ = spawned.child.kill().await;
    }

    #[tokio::test]
    async fn calls_fail_cleanly_while_the_slot_is_empty() {
        let client = StdioClient::new();
        let err = client
            .call_tool("anything", &serde_json::json!({}))
            .await
            .unwrap_err();
        assert_eq!(err, "Backend is not running");
    }
}
