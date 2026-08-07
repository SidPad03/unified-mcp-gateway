//! One task per backend, watching it for as long as the app runs.
//!
//! This is defect #3 from the design doc. The old agent stored the child process
//! as `_child` and never awaited it: a backend that crashed on the first tool
//! call stayed "running" in the UI, kept its tools registered with the gateway,
//! and failed every call until someone restarted the whole agent. A supervisor
//! notices the exit, takes the tools back out of the registration, and brings
//! the process back with a capped backoff.
//!
//! The loop is deliberately one flat state machine rather than a set of
//! callbacks — restart, reload, enable and stop all have to interrupt whatever
//! the backend is doing, including a backoff sleep, and a `select!` over one
//! command channel is the honest way to express that.

use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, ChildStderr};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::backends::{self, Backend, BackendCmd, BackendStatus, Client, CoreHooks};
use crate::config::LocalBackendConfig;
use crate::logbuf::{LogBuffer, LogLevel};

const INITIAL_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(30);
/// A process that stayed up this long is considered healthy, so the next crash
/// starts backing off from one second again rather than from thirty.
const STABLE_AFTER: Duration = Duration::from_secs(60);

pub fn spawn(
    backend: Arc<Backend>,
    rx: mpsc::UnboundedReceiver<BackendCmd>,
    hooks: CoreHooks,
) -> JoinHandle<()> {
    tokio::spawn(run(backend, rx, hooks))
}

/// What a command asked the loop to do next.
enum Next {
    /// Go round again and (re)start the backend.
    Restart,
    /// Leave the loop for good; the backend has been removed or the app is
    /// shutting down.
    Exit,
}

async fn run(backend: Arc<Backend>, mut rx: mpsc::UnboundedReceiver<BackendCmd>, hooks: CoreHooks) {
    let name = backend.name.clone();
    let mut backoff = INITIAL_BACKOFF;

    loop {
        let config = backend.config().await;

        if !config.enabled {
            mark_down(&backend, &hooks, BackendStatus::Disabled, None).await;
            match wait_for_command(&mut rx).await {
                Next::Exit => break,
                Next::Restart => {
                    backoff = INITIAL_BACKOFF;
                    continue;
                }
            }
        }

        backend
            .update_runtime(|r| {
                r.status = BackendStatus::Starting;
                r.error = None;
            })
            .await;
        hooks.touch();

        match start(&backend, &config, &hooks).await {
            Ok(mut running) => {
                let up_since = tokio::time::Instant::now();
                let next = supervise(&mut running, &mut rx).await;
                let ran_for = up_since.elapsed();

                // Whatever happened, this process is finished: stop routing to
                // it before anything else, so an in-flight registration cannot
                // advertise tools that have no process behind them.
                running.shutdown().await;

                match next {
                    Supervised::Exited(status) => {
                        let detail = describe_exit(&status);
                        tracing::warn!(backend = %name, status = %detail, "Backend exited");
                        hooks.logs.push(
                            LogLevel::Error,
                            name.as_str(),
                            format!("Process exited ({detail}); restarting"),
                        );
                        mark_down(
                            &backend,
                            &hooks,
                            BackendStatus::Crashed,
                            Some(format!("Process exited ({detail})")),
                        )
                        .await;
                        backend.update_runtime(|r| r.restarts += 1).await;
                        if ran_for >= STABLE_AFTER {
                            backoff = INITIAL_BACKOFF;
                        }
                    }
                    Supervised::Command(Next::Exit) => {
                        mark_down(&backend, &hooks, BackendStatus::Stopped, None).await;
                        break;
                    }
                    Supervised::Command(Next::Restart) => {
                        mark_down(&backend, &hooks, BackendStatus::Stopped, None).await;
                        // A user-initiated restart should be immediate.
                        backoff = INITIAL_BACKOFF;
                        continue;
                    }
                }
            }
            Err(error) => {
                tracing::error!(backend = %name, %error, "Backend failed to start");
                hooks.logs.push(
                    LogLevel::Error,
                    name.as_str(),
                    format!("Failed to start: {error}"),
                );
                mark_down(&backend, &hooks, BackendStatus::Failed, Some(error)).await;
            }
        }

        // Back off — but stay responsive. Someone who fixes a typo in the
        // command and hits Restart should not wait out a thirty-second sleep.
        tokio::select! {
            _ = tokio::time::sleep(backoff) => {}
            cmd = rx.recv() => {
                match handle_command(cmd, &backend).await {
                    Next::Exit => break,
                    Next::Restart => backoff = INITIAL_BACKOFF,
                }
            }
        }
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }

    tracing::debug!(backend = %name, "Supervisor stopped");
}

// ── Starting ────────────────────────────────────────────────────────────

/// A backend that is up, and everything needed to take it back down.
struct Running {
    backend: Arc<Backend>,
    hooks: CoreHooks,
    child: Option<Child>,
    stderr_pump: Option<JoinHandle<()>>,
}

impl Running {
    async fn shutdown(&mut self) {
        self.backend.set_client(None).await;
        self.hooks.routes.remove_backend(&self.backend.name).await;
        if let Some(mut child) = self.child.take() {
            let _ = child.kill().await;
        }
        if let Some(pump) = self.stderr_pump.take() {
            pump.abort();
        }
        self.hooks.touch();
        self.hooks.request_reregister();
    }
}

enum Supervised {
    Exited(std::io::Result<std::process::ExitStatus>),
    Command(Next),
}

async fn supervise(
    running: &mut Running,
    rx: &mut mpsc::UnboundedReceiver<BackendCmd>,
) -> Supervised {
    match running.child.as_mut() {
        Some(child) => {
            tokio::select! {
                status = child.wait() => Supervised::Exited(status),
                cmd = rx.recv() => {
                    Supervised::Command(handle_command(cmd, &running.backend).await)
                }
            }
        }
        // HTTP backends have no process to outlive; they sit here until told to
        // do something.
        None => Supervised::Command(handle_command(rx.recv().await, &running.backend).await),
    }
}

async fn start(
    backend: &Arc<Backend>,
    config: &LocalBackendConfig,
    hooks: &CoreHooks,
) -> Result<Running, String> {
    if config.is_stdio() {
        start_stdio(backend, config, hooks).await
    } else if config.is_http() {
        start_http(backend, config, hooks).await
    } else {
        Err(format!("Unknown transport '{}'", config.transport))
    }
}

async fn start_stdio(
    backend: &Arc<Backend>,
    config: &LocalBackendConfig,
    hooks: &CoreHooks,
) -> Result<Running, String> {
    let mut spawned = backends::stdio::spawn(config)?;
    let pid = spawned.pid;

    // Start draining stderr before the handshake. A server that writes a banner
    // wider than the pipe buffer would otherwise block on its own stderr and
    // never answer `initialize`.
    let pump = pump_stderr(config.name.clone(), spawned.stderr, hooks.logs.clone());

    let handshake = async {
        spawned
            .session
            .request(
                "initialize",
                backends::initialize_params(),
                backends::stdio::INITIALIZE_TIMEOUT,
            )
            .await?;
        let _ = spawned
            .session
            .notify("notifications/initialized", serde_json::json!({}))
            .await;
        spawned
            .session
            .request(
                "tools/list",
                serde_json::json!({}),
                backends::stdio::LIST_TOOLS_TIMEOUT,
            )
            .await
    }
    .await;

    let listed = match handshake {
        Ok(value) => value,
        Err(error) => {
            let _ = spawned.child.kill().await;
            pump.abort();
            return Err(error);
        }
    };

    let tools = backends::parse_tools(&listed, &config.name);
    let client = backends::stdio::StdioClient::new();
    client.install(spawned.session).await;

    become_ready(backend, hooks, Client::Stdio(client), tools, pid).await;

    Ok(Running {
        backend: backend.clone(),
        hooks: hooks.clone(),
        child: Some(spawned.child),
        stderr_pump: Some(pump),
    })
}

async fn start_http(
    backend: &Arc<Backend>,
    config: &LocalBackendConfig,
    hooks: &CoreHooks,
) -> Result<Running, String> {
    let client = backends::http::HttpClient::new(config)?;
    client.initialize().await?;
    let tools = backends::parse_tools(&client.list_tools().await?, &config.name);

    become_ready(backend, hooks, Client::Http(client), tools, None).await;

    Ok(Running {
        backend: backend.clone(),
        hooks: hooks.clone(),
        child: None,
        stderr_pump: None,
    })
}

async fn become_ready(
    backend: &Arc<Backend>,
    hooks: &CoreHooks,
    client: Client,
    tools: Vec<crate::protocol::ToolInfo>,
    pid: Option<u32>,
) {
    let count = tools.len();
    hooks.routes.replace(&backend.name, &tools).await;
    backend.set_client(Some(client)).await;
    backend
        .update_runtime(|r| {
            r.status = BackendStatus::Ready;
            r.error = None;
            r.pid = pid;
            r.started_at = Some(chrono::Utc::now());
            r.tools = tools;
        })
        .await;

    tracing::info!(backend = %backend.name, tool_count = count, "Backend ready");
    hooks.logs.push(
        LogLevel::Info,
        backend.name.as_str(),
        format!("Ready — {count} tool(s) discovered"),
    );
    hooks.touch();
    hooks.request_reregister();
}

async fn mark_down(
    backend: &Arc<Backend>,
    hooks: &CoreHooks,
    status: BackendStatus,
    error: Option<String>,
) {
    hooks.routes.remove_backend(&backend.name).await;
    backend.set_client(None).await;
    backend
        .update_runtime(|r| {
            r.status = status;
            r.error = error;
            r.pid = None;
            r.started_at = None;
            r.tools.clear();
        })
        .await;
    hooks.touch();
    hooks.request_reregister();
}

// ── Commands ────────────────────────────────────────────────────────────

async fn wait_for_command(rx: &mut mpsc::UnboundedReceiver<BackendCmd>) -> Next {
    loop {
        match rx.recv().await {
            None | Some(BackendCmd::Stop) => return Next::Exit,
            Some(BackendCmd::SetEnabled(false)) => continue,
            Some(BackendCmd::SetEnabled(true))
            | Some(BackendCmd::Restart)
            | Some(BackendCmd::Reload(_)) => return Next::Restart,
        }
    }
}

async fn handle_command(cmd: Option<BackendCmd>, backend: &Arc<Backend>) -> Next {
    match cmd {
        // The sender is dropped only when the backend has been removed from the
        // manager, so there is nothing left to supervise.
        None | Some(BackendCmd::Stop) => Next::Exit,
        Some(BackendCmd::Restart) => Next::Restart,
        Some(BackendCmd::Reload(config)) => {
            backend.set_config(*config).await;
            Next::Restart
        }
        // Both directions come back through the top of the loop, which reads
        // `enabled` from the config and does the right thing.
        Some(BackendCmd::SetEnabled(_)) => Next::Restart,
    }
}

// ── stderr → Logs page ──────────────────────────────────────────────────

fn pump_stderr(name: String, stderr: ChildStderr, logs: Arc<LogBuffer>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        // The loop ends at EOF, or when the pipe breaks because the process is
        // gone. Either way the supervisor's `child.wait()` is what reacts to it.
        while let Ok(Some(line)) = lines.next_line().await {
            let trimmed = line.trim_end();
            if trimmed.is_empty() {
                continue;
            }
            logs.push(LogLevel::guess_from_stderr(trimmed), name.as_str(), trimmed);
        }
    })
}

fn describe_exit(status: &std::io::Result<std::process::ExitStatus>) -> String {
    match status {
        Ok(status) => {
            #[cfg(unix)]
            {
                use std::os::unix::process::ExitStatusExt;
                if let Some(signal) = status.signal() {
                    return format!("killed by signal {signal}");
                }
            }
            match status.code() {
                Some(code) => format!("exit code {code}"),
                None => "terminated".to_string(),
            }
        }
        Err(e) => format!("could not be waited on: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::{BackendManager, RouteTable};
    use std::sync::atomic::AtomicU64;

    fn hooks() -> (CoreHooks, mpsc::Receiver<()>) {
        let (tx, rx) = mpsc::channel(1);
        (
            CoreHooks {
                logs: Arc::new(LogBuffer::new()),
                routes: Arc::new(RouteTable::default()),
                reregister: tx,
                generation: Arc::new(AtomicU64::new(0)),
            },
            rx,
        )
    }

    /// A minimal MCP server in `sh`: answers initialize and tools/list, then
    /// does whatever `after` says.
    fn fake_server(name: &str, after: &str) -> LocalBackendConfig {
        let script = format!(
            r#"
            reply() {{
              id=$(printf '%s' "$1" | sed 's/.*"id":"\([^"]*\)".*/\1/')
              printf '{{"jsonrpc":"2.0","id":"%s","result":%s}}\n' "$id" "$2"
            }}
            read -r line; reply "$line" '{{"protocolVersion":"2025-03-26"}}'
            while read -r line; do
              case "$line" in
                *tools/list*) reply "$line" '{{"tools":[{{"name":"ping","description":"p"}}]}}'; break ;;
              esac
            done
            {after}
            "#
        );
        LocalBackendConfig {
            name: name.into(),
            transport: "stdio".into(),
            command: Some("/bin/sh".into()),
            args: vec!["-c".into(), script],
            ..Default::default()
        }
    }

    async fn wait_for_status(
        manager: &Arc<BackendManager>,
        name: &str,
        want: BackendStatus,
    ) -> bool {
        for _ in 0..200 {
            let views = manager.snapshot().await;
            if views.iter().any(|v| v.name == name && v.status == want) {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        false
    }

    #[tokio::test]
    async fn a_backend_that_comes_up_registers_its_tools() {
        let (hooks, _rx) = hooks();
        let manager = BackendManager::new(hooks);
        manager.add(fake_server("probe", "sleep 30")).await.unwrap();

        assert!(
            wait_for_status(&manager, "probe", BackendStatus::Ready).await,
            "backend never became ready"
        );
        assert_eq!(
            manager.route_of("probe__ping").await.as_deref(),
            Some("probe")
        );
        assert_eq!(manager.ready_tools().await.len(), 1);
        manager.shutdown().await;
    }

    #[tokio::test]
    async fn a_crashed_backend_is_noticed_and_its_tools_withdrawn() {
        // Defect #3 and #10 together: the process goes away, and the tools must
        // stop being advertised rather than sitting there failing every call.
        let (hooks, _rx) = hooks();
        let manager = BackendManager::new(hooks);
        // The `sleep 1` is not padding: without it the process exits in the same
        // millisecond it becomes ready, and the test cannot observe the
        // transition it is about to assert on.
        manager
            .add(fake_server("flaky", "sleep 1; exit 1"))
            .await
            .unwrap();

        assert!(wait_for_status(&manager, "flaky", BackendStatus::Ready).await);
        assert!(
            wait_for_status(&manager, "flaky", BackendStatus::Crashed).await,
            "the exit was never noticed"
        );
        assert_eq!(manager.route_of("flaky__ping").await, None);
        assert!(manager.ready_tools().await.is_empty());
        manager.shutdown().await;
    }

    #[tokio::test]
    async fn a_crashed_backend_is_restarted() {
        let (hooks, _rx) = hooks();
        let manager = BackendManager::new(hooks);
        manager
            .add(fake_server("flaky", "sleep 1; exit 1"))
            .await
            .unwrap();

        assert!(wait_for_status(&manager, "flaky", BackendStatus::Crashed).await);
        // The first backoff is one second, so it comes back on its own.
        assert!(
            wait_for_status(&manager, "flaky", BackendStatus::Ready).await,
            "the backend was never restarted"
        );
        let view = manager
            .snapshot()
            .await
            .into_iter()
            .find(|v| v.name == "flaky")
            .unwrap();
        assert!(view.restarts >= 1, "restarts were not counted");
        manager.shutdown().await;
    }

    #[tokio::test]
    async fn one_failing_backend_does_not_stop_the_others() {
        // Defect #2: `start_all` used to return Err on the first failure and the
        // whole agent exited.
        let (hooks, _rx) = hooks();
        let manager = BackendManager::new(hooks);
        manager
            .start_all(&[
                LocalBackendConfig {
                    name: "broken".into(),
                    transport: "stdio".into(),
                    command: Some("definitely-not-a-real-binary-xyz".into()),
                    ..Default::default()
                },
                fake_server("good", "sleep 30"),
            ])
            .await;

        assert!(wait_for_status(&manager, "good", BackendStatus::Ready).await);
        let broken = manager
            .snapshot()
            .await
            .into_iter()
            .find(|v| v.name == "broken")
            .unwrap();
        assert_eq!(broken.status, BackendStatus::Failed);
        assert!(broken.error.is_some());
        manager.shutdown().await;
    }

    #[tokio::test]
    async fn a_disabled_backend_is_never_spawned() {
        let (hooks, _rx) = hooks();
        let manager = BackendManager::new(hooks);
        let mut config = fake_server("off", "sleep 30");
        config.enabled = false;
        manager.add(config).await.unwrap();

        assert!(wait_for_status(&manager, "off", BackendStatus::Disabled).await);
        assert!(manager.ready_tools().await.is_empty());

        // ...and enabling it brings it up without a restart of the app.
        manager.set_enabled("off", true).await.unwrap();
        assert!(wait_for_status(&manager, "off", BackendStatus::Ready).await);
        manager.shutdown().await;
    }

    #[tokio::test]
    async fn a_backend_can_be_removed_while_running() {
        // Defect #5.
        let (hooks, _rx) = hooks();
        let manager = BackendManager::new(hooks);
        manager.add(fake_server("temp", "sleep 30")).await.unwrap();
        assert!(wait_for_status(&manager, "temp", BackendStatus::Ready).await);

        manager.remove("temp").await.unwrap();
        assert!(manager.snapshot().await.is_empty());
        assert_eq!(manager.route_of("temp__ping").await, None);
    }

    #[tokio::test]
    async fn stderr_from_a_backend_reaches_the_log_buffer() {
        // Defect #4: stderr used to be Stdio::inherit(), which in an app goes
        // nowhere at all.
        let (hooks, _rx) = hooks();
        let logs = hooks.logs.clone();
        let manager = BackendManager::new(hooks);
        manager
            .add(fake_server(
                "noisy",
                "echo 'WARNING: low on memory' >&2; sleep 30",
            ))
            .await
            .unwrap();

        assert!(wait_for_status(&manager, "noisy", BackendStatus::Ready).await);
        for _ in 0..40 {
            if logs
                .snapshot()
                .iter()
                .any(|l| l.source == "noisy" && l.message.contains("low on memory"))
            {
                manager.shutdown().await;
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("backend stderr never reached the Logs page");
    }

    #[test]
    fn exit_descriptions_name_the_cause() {
        use std::os::unix::process::ExitStatusExt;
        let signalled = std::process::ExitStatus::from_raw(9);
        assert!(describe_exit(&Ok(signalled)).contains("signal"));
    }
}
