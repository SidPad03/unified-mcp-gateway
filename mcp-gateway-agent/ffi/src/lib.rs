//! The C ABI the SwiftUI app links against.
//!
//! Deliberately four functions and one callback. Everything interesting is JSON
//! in and JSON out, because a wide FFI surface is a wide surface to get wrong,
//! and because the Rust side already has to serialize its state for the UI
//! anyway.
//!
//! ```text
//!   Swift                                   Rust
//!   ─────                                   ────
//!   mcpga_start(path, cb, ctx)  ──────────► build runtime, load config,
//!                                           start backends + tunnel
//!   mcpga_command(json) ─────────────────► block_on(handle(cmd)) ──► json
//!   cb(ctx, json)       ◄────────────────── one emitter task, every 100 ms
//!   mcpga_shutdown()    ──────────────────► stop backends, drop runtime
//! ```
//!
//! **Threading contract.** The callback is invoked from exactly one task, so
//! Swift never sees two events at once and does not need a lock — but it *is*
//! a tokio worker thread, not the main thread, so the Swift side hops to the
//! main actor before touching any UI state. `mcpga_command` blocks the calling
//! thread for as long as the command takes (a `test_backend` can take
//! thirty seconds), so Swift calls it off the main thread.
//!
//! **Ownership.** Every `char *` returned by this library was allocated by it
//! and must be handed back to `mcpga_string_free`. Strings passed *in* are
//! borrowed for the duration of the call and never retained.

use std::ffi::{c_char, c_void, CStr, CString};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use mcp_gateway_agent_core as core;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::runtime::Runtime;

/// How often deltas are pushed to the UI. Ten a second is past the point a
/// person can see, and it turns an unbounded stream of log lines into a bounded
/// stream of messages.
const TICK: Duration = Duration::from_millis(100);

struct Bridge {
    runtime: Runtime,
    state: Arc<core::AgentState>,
}

static BRIDGE: OnceLock<Bridge> = OnceLock::new();

// ── Event sink ──────────────────────────────────────────────────────────

/// `void (*)(void *ctx, const char *event_json)`
pub type EventCallback = Option<unsafe extern "C" fn(*mut c_void, *const c_char)>;

/// The callback plus its context, in a form that can cross a thread boundary.
///
/// Safety rests on the Swift side: `ctx` is a pointer to an object that lives
/// until `mcpga_shutdown`, and the callback only reads it.
struct EventSink {
    callback: EventCallback,
    ctx: *mut c_void,
}

unsafe impl Send for EventSink {}
unsafe impl Sync for EventSink {}

impl EventSink {
    fn emit(&self, payload: &str) {
        let Some(callback) = self.callback else {
            return;
        };
        let Ok(text) = CString::new(payload) else {
            // A NUL inside the payload should be impossible — it is JSON built
            // by serde — but silently corrupting the stream would be worse than
            // dropping one tick.
            return;
        };
        // SAFETY: `callback` and `ctx` were handed to us by `mcpga_start` and
        // the Swift side keeps the referenced object alive until shutdown. The
        // pointer is valid only for the duration of the call, which is the
        // documented contract.
        unsafe { callback(self.ctx, text.as_ptr()) }
    }
}

#[derive(Serialize)]
struct Tick {
    #[serde(skip_serializing_if = "Option::is_none")]
    snapshot: Option<core::state::Snapshot>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    logs: Vec<core::LogLine>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    calls: Vec<core::ToolCall>,
}

/// The one task that talks to Swift.
///
/// Coalescing lives here rather than at the call sites: a backend that writes
/// ten thousand stderr lines in a second produces ten messages, not ten
/// thousand, and the snapshot is only re-sent when the generation counter says
/// something actually changed.
async fn emitter(state: Arc<core::AgentState>, sink: EventSink) {
    let mut ticker = tokio::time::interval(TICK);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    let mut log_cursor = 0u64;
    let mut call_cursor = 0u64;
    let last_generation = AtomicU64::new(0);

    loop {
        ticker.tick().await;

        let (logs, next_logs) = state.logs.since(log_cursor);
        let (calls, next_calls) = state.calls.since(call_cursor);
        log_cursor = next_logs;
        call_cursor = next_calls;

        let generation = state.generation();
        let snapshot = if generation != last_generation.swap(generation, Ordering::Relaxed) {
            Some(state.snapshot().await)
        } else {
            None
        };

        if snapshot.is_none() && logs.is_empty() && calls.is_empty() {
            continue;
        }

        let tick = Tick {
            snapshot,
            logs,
            calls,
        };
        if let Ok(payload) = serde_json::to_string(&tick) {
            sink.emit(&payload);
        }
    }
}

// ── Commands ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
enum Command {
    /// Full state. The UI asks for this once on launch; after that it lives off
    /// the deltas.
    Snapshot,
    LogsSnapshot,
    CallsSnapshot,
    ClearLogs,

    SetApiKey {
        key: String,
    },
    /// Move a plaintext key out of an old `config.toml`. Returns the key so the
    /// app can put it in the Keychain, and leaves the config without it.
    TakeLegacyApiKey,

    ApplySettings {
        agent_id: String,
        gateway_url: String,
        #[serde(default)]
        dashboard_url: Option<String>,
        #[serde(default)]
        tls_skip_verify: bool,
    },
    CheckGateway {
        gateway_url: String,
        api_key: String,
        #[serde(default)]
        tls_skip_verify: bool,
    },

    AddBackend {
        backend: core::LocalBackendConfig,
    },
    UpdateBackend {
        name: String,
        backend: core::LocalBackendConfig,
    },
    RemoveBackend {
        name: String,
    },
    RestartBackend {
        name: String,
    },
    SetBackendEnabled {
        name: String,
        enabled: bool,
    },
    TestBackend {
        backend: core::LocalBackendConfig,
    },

    Reconnect,
    Reregister,
    Shutdown,
}

async fn handle(state: &Arc<core::AgentState>, command: Command) -> Result<Value, String> {
    match command {
        Command::Snapshot => Ok(serde_json::to_value(state.snapshot().await).unwrap()),
        Command::LogsSnapshot => Ok(json!({
            "lines": state.logs.snapshot(),
            "dropped": state.logs.dropped(),
        })),
        Command::CallsSnapshot => Ok(json!({ "calls": state.calls.snapshot() })),
        Command::ClearLogs => {
            state.logs.clear();
            Ok(Value::Null)
        }

        Command::SetApiKey { key } => {
            state.set_api_key(key).await;
            Ok(Value::Null)
        }
        Command::TakeLegacyApiKey => match state.take_legacy_api_key().await {
            Some(key) => {
                // Rewrite the config without it before handing it over, so a
                // crash between here and the Keychain write cannot leave the
                // key in two places.
                state.persist().await.map_err(|e| e.to_string())?;
                Ok(json!({ "key": key }))
            }
            None => Ok(json!({ "key": Value::Null })),
        },

        Command::ApplySettings {
            agent_id,
            gateway_url,
            dashboard_url,
            tls_skip_verify,
        } => {
            state
                .apply_settings(
                    agent_id,
                    core::config::normalize_gateway_url(&gateway_url),
                    dashboard_url,
                    tls_skip_verify,
                )
                .await
                .map_err(|e| e.to_string())?;
            Ok(Value::Null)
        }
        Command::CheckGateway {
            gateway_url,
            api_key,
            tls_skip_verify,
        } => {
            let normalized = core::config::normalize_gateway_url(&gateway_url);
            let check = core::tunnel::check_gateway(&normalized, &api_key, tls_skip_verify).await;
            let mut value = serde_json::to_value(check).unwrap();
            value["normalized_url"] = json!(normalized);
            Ok(value)
        }

        Command::AddBackend { backend } => {
            state.add_backend(backend).await?;
            Ok(Value::Null)
        }
        Command::UpdateBackend { name, backend } => {
            state.update_backend(&name, backend).await?;
            Ok(Value::Null)
        }
        Command::RemoveBackend { name } => {
            state.remove_backend(&name).await?;
            Ok(Value::Null)
        }
        Command::RestartBackend { name } => {
            state.backends.restart(&name).await?;
            Ok(Value::Null)
        }
        Command::SetBackendEnabled { name, enabled } => {
            state.set_backend_enabled(&name, enabled).await?;
            Ok(Value::Null)
        }
        Command::TestBackend { backend } => {
            Ok(serde_json::to_value(core::backends::test_connection(&backend).await?).unwrap())
        }

        Command::Reconnect => {
            state.request_reconnect();
            Ok(Value::Null)
        }
        Command::Reregister => {
            state.request_reregister();
            Ok(Value::Null)
        }
        Command::Shutdown => {
            state.shutdown().await;
            Ok(Value::Null)
        }
    }
}

// ── Exported functions ──────────────────────────────────────────────────

/// Start the agent. Returns 0 on success, non-zero on failure.
///
/// # Safety
/// `config_path` must be a valid NUL-terminated UTF-8 string, or null to use
/// the default (`~/.mcp-gateway-agent/config.toml`). `ctx` must remain valid
/// until [`mcpga_shutdown`] returns.
#[no_mangle]
pub unsafe extern "C" fn mcpga_start(
    config_path: *const c_char,
    callback: EventCallback,
    ctx: *mut c_void,
) -> i32 {
    if BRIDGE.get().is_some() {
        return 0; // already running; starting twice is a no-op, not an error
    }

    let path = match borrow_str(config_path) {
        Some(p) if !p.trim().is_empty() => std::path::PathBuf::from(p),
        _ => core::config::default_config_path(),
    };

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("mcp-gateway-agent")
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => return 1,
    };

    if core::config::ensure_dirs().is_err() {
        return 2;
    }

    let config = match core::config::load(&path) {
        Ok(config) => config,
        // A corrupt config must not stop the app from launching — the user
        // needs the UI to fix it. Start empty and let the wizard take over.
        Err(e) => {
            eprintln!("mcp-gateway-agent: {e}");
            core::Config::default()
        }
    };

    let state = runtime.block_on(async {
        let state = core::AgentState::new(path, config);
        install_tracing(state.logs.clone());
        state.start().await;
        state
    });

    let sink = EventSink { callback, ctx };
    runtime.spawn(emitter(state.clone(), sink));

    let _ = BRIDGE.set(Bridge { runtime, state });
    0
}

/// Run one command. Returns a JSON envelope the caller must free with
/// [`mcpga_string_free`].
///
/// # Safety
/// `request_json` must be a valid NUL-terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn mcpga_command(request_json: *const c_char) -> *mut c_char {
    let Some(bridge) = BRIDGE.get() else {
        return into_c_string(&envelope(Err("The agent is not running".into())));
    };
    let Some(raw) = borrow_str(request_json) else {
        return into_c_string(&envelope(Err("Command was not valid UTF-8".into())));
    };

    let result = match serde_json::from_str::<Command>(raw) {
        Ok(command) => bridge.runtime.block_on(handle(&bridge.state, command)),
        Err(e) => Err(format!("Could not parse the command: {e}")),
    };

    into_c_string(&envelope(result))
}

/// Free a string returned by this library.
///
/// # Safety
/// `ptr` must be a pointer previously returned by [`mcpga_command`] or
/// [`mcpga_version`], and must not be used afterwards.
#[no_mangle]
pub unsafe extern "C" fn mcpga_string_free(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(CString::from_raw(ptr));
    }
}

/// Stop the local backends and let the tunnel go.
///
/// The runtime itself is deliberately left standing: the process is about to
/// exit, and tearing down a runtime from inside a callback it owns is a good
/// way to deadlock on the way out.
#[no_mangle]
pub extern "C" fn mcpga_shutdown() {
    if let Some(bridge) = BRIDGE.get() {
        bridge.runtime.block_on(bridge.state.shutdown());
    }
}

/// Version of the agent core. Free with [`mcpga_string_free`].
#[no_mangle]
pub extern "C" fn mcpga_version() -> *mut c_char {
    into_c_string(core::VERSION)
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn envelope(result: Result<Value, String>) -> String {
    let value = match result {
        Ok(data) => json!({ "ok": true, "data": data }),
        Err(error) => json!({ "ok": false, "error": error }),
    };
    // Serializing a `Value` cannot fail.
    serde_json::to_string(&value).unwrap_or_else(|_| {
        r#"{"ok":false,"error":"Could not serialize the response"}"#.to_string()
    })
}

fn into_c_string(text: &str) -> *mut c_char {
    match CString::new(text) {
        Ok(owned) => owned.into_raw(),
        Err(_) => CString::new(r#"{"ok":false,"error":"Response contained a NUL byte"}"#)
            .expect("literal has no NUL")
            .into_raw(),
    }
}

/// # Safety
/// `ptr` must be null or a valid NUL-terminated string.
unsafe fn borrow_str<'a>(ptr: *const c_char) -> Option<&'a str> {
    if ptr.is_null() {
        return None;
    }
    CStr::from_ptr(ptr).to_str().ok()
}

/// Route `tracing` into the ring buffer that feeds the Logs page.
///
/// There is no terminal under an `.app`, so without this every `tracing::warn!`
/// in the agent would go to a stderr nobody reads.
fn install_tracing(logs: Arc<core::LogBuffer>) {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        tracing_subscriber::EnvFilter::new("mcp_gateway_agent_core=info,mcp_gateway_agent_ffi=info")
    });

    // `try_init` rather than `init`: a second call (a test, or a restart)
    // should not panic the app.
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(core::logbuf::LogLayer::new(logs))
        .try_init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_successful_command_is_wrapped_in_an_ok_envelope() {
        let text = envelope(Ok(json!({"a": 1})));
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["ok"], true);
        assert_eq!(value["data"]["a"], 1);
    }

    #[test]
    fn a_failure_carries_the_message_not_a_code() {
        let text = envelope(Err("No backend named 'ghost'".into()));
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["ok"], false);
        assert_eq!(value["error"], "No backend named 'ghost'");
    }

    #[test]
    fn every_command_the_app_sends_parses() {
        let commands = [
            r#"{"cmd":"snapshot"}"#,
            r#"{"cmd":"logs_snapshot"}"#,
            r#"{"cmd":"calls_snapshot"}"#,
            r#"{"cmd":"clear_logs"}"#,
            r#"{"cmd":"set_api_key","key":"mcpgw_x"}"#,
            r#"{"cmd":"take_legacy_api_key"}"#,
            r#"{"cmd":"apply_settings","agent_id":"mac","gateway_url":"wss://gw/agent/ws"}"#,
            r#"{"cmd":"check_gateway","gateway_url":"wss://gw","api_key":"k","tls_skip_verify":true}"#,
            r#"{"cmd":"add_backend","backend":{"name":"b","transport":"stdio","command":"uvx"}}"#,
            r#"{"cmd":"update_backend","name":"b","backend":{"name":"b","transport":"stdio","command":"uvx"}}"#,
            r#"{"cmd":"remove_backend","name":"b"}"#,
            r#"{"cmd":"restart_backend","name":"b"}"#,
            r#"{"cmd":"set_backend_enabled","name":"b","enabled":false}"#,
            r#"{"cmd":"test_backend","backend":{"name":"b","transport":"http","url":"http://x/mcp"}}"#,
            r#"{"cmd":"reconnect"}"#,
            r#"{"cmd":"reregister"}"#,
            r#"{"cmd":"shutdown"}"#,
        ];
        for raw in commands {
            serde_json::from_str::<Command>(raw)
                .unwrap_or_else(|e| panic!("{raw} failed to parse: {e}"));
        }
    }

    #[test]
    fn an_unknown_command_is_an_error_not_a_panic() {
        assert!(serde_json::from_str::<Command>(r#"{"cmd":"drop_database"}"#).is_err());
    }

    #[test]
    fn strings_round_trip_through_the_boundary() {
        let ptr = into_c_string(r#"{"ok":true}"#);
        // SAFETY: just allocated by `into_c_string`.
        unsafe {
            assert_eq!(CStr::from_ptr(ptr).to_str().unwrap(), r#"{"ok":true}"#);
            mcpga_string_free(ptr);
        }
    }

    #[test]
    fn commanding_a_bridge_that_was_never_started_says_so() {
        // SAFETY: a valid NUL-terminated literal.
        let response = unsafe {
            let request = CString::new(r#"{"cmd":"snapshot"}"#).unwrap();
            let ptr = mcpga_command(request.as_ptr());
            let text = CStr::from_ptr(ptr).to_str().unwrap().to_string();
            mcpga_string_free(ptr);
            text
        };
        let value: Value = serde_json::from_str(&response).unwrap();
        assert_eq!(value["ok"], false);
    }
}
