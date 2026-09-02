//! The agent's own tools — the `agent_*` namespace.
//!
//! Everything else this agent registers with the gateway belongs to a local MCP
//! server: it was discovered on this Mac, and the agent routes to it. These
//! tools have no server behind them. They *are* the agent, offered to whoever
//! is on the other end of the tunnel so that "install and expose the Obsidian
//! MCP server" is something an assistant can do rather than something a person
//! has to do in this app.
//!
//! They are a smaller mirror of the gateway's own `gateway_*` namespace, scoped
//! to this machine, and the same three rules hold:
//!
//! 1. **They are classified.** The gateway files each one under a risk category
//!    — installing is `admin`, removing is `destructive` — so the operator's
//!    existing RBAC governs them without knowing they are special. The mapping
//!    lives in the server's `backends::classifier`.
//! 2. **They are audited.** They arrive as ordinary tool calls, so the gateway
//!    records them in the same trail as everything else, and this agent's own
//!    Activity page shows them alongside real tool traffic.
//! 3. **They can be switched off.** `agent.expose_control_tools` in
//!    `config.toml`, and the toggle in Settings, decide whether they are
//!    registered at all. A machine that has them off cannot be configured from
//!    the gateway, only used.
//!
//! Deliberately absent: anything that changes the tunnel itself. A call that
//! repointed `gateway_url` would arrive over the connection it was about to
//! sever, and the answer would never get back — so the gateway address, the
//! agent id and the TLS setting stay in this app, where a person can see what
//! they are doing.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{json, Map, Value};

use crate::backends::test_connection;
use crate::config::LocalBackendConfig;
use crate::logbuf::LogLevel;
use crate::protocol::ToolInfo;
use crate::state::AgentState;

fn schema(properties: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
    })
}

fn server_prop(what: &str) -> Value {
    json!({ "type": "string", "description": what })
}

/// The configuration fields an install or an update may set, shared so the two
/// tools cannot drift apart.
fn config_props() -> Map<String, Value> {
    json!({
        "command": { "type": "string", "description": "stdio only — the executable, e.g. 'uvx' or 'npx'." },
        "args": { "type": "array", "items": { "type": "string" }, "description": "stdio only." },
        "env": { "type": "object", "description": "stdio only — environment for the child process. Replaces the whole block." },
        "url": { "type": "string", "description": "http only — the MCP endpoint." },
        "headers": { "type": "object", "description": "http only — sent on every request. Replaces the whole block." },
        "masked": {
            "type": "array",
            "items": { "type": "string" },
            "description": "Keys of env or headers whose values must never be displayed again — here, or on the gateway. Set this for tokens."
        }
    })
    .as_object()
    .cloned()
    .unwrap_or_default()
}

fn with_config_props(mut extra: Map<String, Value>) -> Value {
    let mut props = config_props();
    props.append(&mut extra);
    Value::Object(props)
}

/// Every tool in the namespace, as the gateway will register them.
///
/// Names are bare here; the gateway namespaces them under this agent's id, so
/// they arrive at a client as `<agent-id>__agent_list_local_servers`.
pub fn catalog() -> Vec<ToolInfo> {
    let tool = |name: &str, description: &str, input_schema: Value| ToolInfo {
        name: name.to_string(),
        description: description.to_string(),
        input_schema,
    };

    vec![
        tool(
            "agent_list_local_servers",
            "Every MCP server configured on this Mac, with its transport, whether it is running, \
             how many tools it exposes and the names — never the values — of its environment.",
            schema(json!({}), &[]),
        ),
        tool(
            "agent_install_mcp_server",
            "Add an MCP server to this Mac and expose it through the gateway. The command is \
             started and asked for its tool list first; if it does not come up, nothing is \
             written and the error is returned instead.",
            Value::Object({
                let mut props = config_props();
                props.insert(
                    "name".into(),
                    json!({
                        "type": "string",
                        "description": "Unique on this Mac. Becomes the tool namespace: '<name>__<tool>'. No spaces, and no '__'."
                    }),
                );
                props.insert(
                    "transport".into(),
                    json!({ "type": "string", "enum": ["stdio", "http", "streamable-http"], "description": "Defaults to stdio." }),
                );
                props.insert(
                    "enabled".into(),
                    json!({ "type": "boolean", "description": "Whether to expose it immediately. Defaults to true." }),
                );
                let mut map = Map::new();
                map.insert("type".into(), json!("object"));
                map.insert("properties".into(), Value::Object(props));
                map.insert("required".into(), json!(["name"]));
                map
            }),
        ),
        tool(
            "agent_remove_mcp_server",
            "Remove an MCP server from this Mac: stop it, withdraw its tools from the gateway, \
             and delete its configuration. Cannot be undone.",
            schema(
                json!({ "name": server_prop("The server to remove.") }),
                &["name"],
            ),
        ),
        tool(
            "agent_start_local_server",
            "Enable a configured MCP server and start it, registering its tools with the gateway.",
            schema(
                json!({ "name": server_prop("The server to start.") }),
                &["name"],
            ),
        ),
        tool(
            "agent_stop_local_server",
            "Stop an MCP server and disable it. Its process is killed and its tools are withdrawn \
             from the gateway, so every client loses them until it is started again.",
            schema(
                json!({ "name": server_prop("The server to stop.") }),
                &["name"],
            ),
        ),
        tool(
            "agent_restart_local_server",
            "Restart an MCP server and re-discover its tools. Use it after changing a server's \
             configuration outside this agent, or to clear a wedged process.",
            schema(
                json!({ "name": server_prop("The server to restart.") }),
                &["name"],
            ),
        ),
        tool(
            "agent_get_local_server_status",
            "Whether a server is running, since when, its process id, how many times it has been \
             restarted, and the error it failed with. Omit the name for every server at once.",
            schema(json!({ "name": server_prop("Omit for all servers.") }), &[]),
        ),
        tool(
            "agent_get_local_server_logs",
            "Tail what a server has written to stderr since the agent started it. Omit the name \
             for the agent's own log, which covers connection and supervision.",
            schema(
                json!({
                    "name": server_prop("Omit for the agent's own log."),
                    "lines": { "type": "integer", "description": "How many of the most recent lines. Default 100, maximum 500." },
                    "min_level": { "type": "string", "enum": ["trace", "debug", "info", "warn", "error"], "description": "Drop anything below this level." }
                }),
                &[],
            ),
        ),
        tool(
            "agent_update_config",
            "Change a configured MCP server: its command, arguments, environment, headers, which \
             values are masked, and whether it is exposed to the gateway. The server is restarted \
             under the new configuration. This Mac's connection settings — the gateway address, \
             the agent id — are not changeable here; a call that repointed the tunnel would \
             arrive over the connection it was severing.",
            Value::Object({
                let props = with_config_props(
                    json!({
                        "server": { "type": "string", "description": "The server to change." },
                        "enabled": { "type": "boolean", "description": "Whether it is exposed to the gateway." }
                    })
                    .as_object()
                    .cloned()
                    .unwrap_or_default(),
                );
                let mut map = Map::new();
                map.insert("type".into(), json!("object"));
                map.insert("properties".into(), props);
                map.insert("required".into(), json!(["server"]));
                map
            }),
        ),
    ]
}

pub fn is_control_tool(name: &str) -> bool {
    catalog().iter().any(|t| t.name == name)
}

// ── Argument helpers ────────────────────────────────────────────────────

fn req_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("'{key}' is required"))
}

fn opt_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn opt_strings(args: &Value, key: &str) -> Option<Vec<String>> {
    args.get(key).and_then(Value::as_array).map(|a| {
        a.iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect()
    })
}

fn opt_string_map(args: &Value, key: &str) -> Option<HashMap<String, String>> {
    args.get(key).and_then(Value::as_object).map(|o| {
        o.iter()
            .filter_map(|(k, v)| match v {
                Value::String(s) => Some((k.clone(), s.clone())),
                // A number or a bool in an environment is almost certainly a
                // typo'd string; carry it rather than silently dropping it.
                Value::Null => None,
                other => Some((k.clone(), other.to_string())),
            })
            .collect()
    })
}

fn opt_i64(args: &Value, key: &str) -> Option<i64> {
    args.get(key).and_then(Value::as_i64)
}

/// Fold the flat tool arguments onto a backend configuration.
///
/// `base` is what is on disk, so an update that only changes `args` keeps the
/// environment. A caller that echoes back the mask placeholder it was shown
/// gets the stored value restored by `AgentState::resolve_masked` further down
/// the line, which is the same round trip the app's editor makes.
fn apply_config(args: &Value, mut config: LocalBackendConfig) -> LocalBackendConfig {
    if let Some(command) = opt_str(args, "command") {
        config.command = Some(command.to_string());
    }
    if let Some(list) = opt_strings(args, "args") {
        config.args = list;
    }
    if let Some(env) = opt_string_map(args, "env") {
        config.env = env;
    }
    if let Some(url) = opt_str(args, "url") {
        config.url = Some(url.to_string());
    }
    if let Some(headers) = opt_string_map(args, "headers") {
        config.headers = headers;
    }
    // One `masked` list from the caller, landing on whichever block this
    // transport actually has — a caller never has to know the difference
    // between `masked_env` and `masked_headers`.
    if let Some(masked) = opt_strings(args, "masked") {
        if config.is_stdio() {
            config.masked_env = masked;
        } else {
            config.masked_headers = masked;
        }
    }
    if let Some(enabled) = args.get("enabled").and_then(Value::as_bool) {
        config.enabled = enabled;
    }
    config.tidy_masks();
    config
}

// ── Dispatch ────────────────────────────────────────────────────────────

/// Run one control tool.
pub async fn call(state: &Arc<AgentState>, name: &str, args: &Value) -> Result<Value, String> {
    match name {
        "agent_list_local_servers" => list_servers(state).await,
        "agent_install_mcp_server" => install_server(state, args).await,
        "agent_remove_mcp_server" => remove_server(state, args).await,
        "agent_start_local_server" => set_running(state, args, true).await,
        "agent_stop_local_server" => set_running(state, args, false).await,
        "agent_restart_local_server" => restart_server(state, args).await,
        "agent_get_local_server_status" => server_status(state, args).await,
        "agent_get_local_server_logs" => server_logs(state, args).await,
        "agent_update_config" => update_config(state, args).await,
        other => Err(format!("Unknown agent tool: {other}")),
    }
}

async fn list_servers(state: &Arc<AgentState>) -> Result<Value, String> {
    let servers: Vec<Value> = state
        .backends
        .snapshot()
        .await
        .into_iter()
        .map(|b| {
            json!({
                "name": b.name,
                "transport": b.transport,
                "enabled": b.enabled,
                "status": b.status,
                "tool_count": b.tool_count,
                "command": b.command,
                "args": b.args,
                "url": b.url,
                // Names only. The values are the entire reason a server has an
                // environment block, and they do not leave this Mac.
                "env_keys": b.env.iter().map(|e| &e.key).collect::<Vec<_>>(),
                "masked_keys": b
                    .env
                    .iter()
                    .chain(b.headers.iter())
                    .filter(|e| e.masked)
                    .map(|e| &e.key)
                    .collect::<Vec<_>>(),
                "header_keys": b.headers.iter().map(|e| &e.key).collect::<Vec<_>>(),
                "error": b.error,
            })
        })
        .collect();

    Ok(json!({ "count": servers.len(), "servers": servers }))
}

async fn install_server(state: &Arc<AgentState>, args: &Value) -> Result<Value, String> {
    let name = req_str(args, "name")?;
    let transport = opt_str(args, "transport").unwrap_or("stdio");
    if !["stdio", "http", "streamable-http"].contains(&transport) {
        return Err(format!(
            "'transport' must be 'stdio', 'http' or 'streamable-http', not '{transport}'"
        ));
    }
    if state.config().await.backend(name).is_some() {
        return Err(format!(
            "A server named '{name}' is already installed. Change it with agent_update_config."
        ));
    }

    let config = apply_config(
        args,
        LocalBackendConfig {
            name: name.to_string(),
            transport: transport.to_string(),
            ..Default::default()
        },
    );
    // `validate` catches the empty command / bad URL cases with the same
    // messages the app's editor shows.
    config.validate()?;

    // Prove it starts before adopting it. Installing something that cannot run
    // would leave a broken row on the gateway and a puzzle for whoever looks
    // next; the app's Add sheet has always tested first, and so does this.
    let probe = test_connection(&config)
        .await
        .map_err(|e| format!("'{name}' did not start, so nothing was installed: {e}"))?;

    state.add_backend(config).await?;

    Ok(json!({
        "name": name,
        "transport": transport,
        "installed": true,
        "tool_count": probe.tool_count,
        "tools": probe.tools,
        "took_ms": probe.took_ms,
        "namespace": format!("{name}__*"),
        "note": "Registered with the gateway within a second or so; the tools appear there under this Mac's agent.",
    }))
}

async fn remove_server(state: &Arc<AgentState>, args: &Value) -> Result<Value, String> {
    let name = req_str(args, "name")?;
    state.remove_backend(name).await?;
    Ok(json!({
        "name": name,
        "removed": true,
        "note": "Its process is stopped, its tools are withdrawn from the gateway, and its configuration is deleted.",
    }))
}

async fn set_running(
    state: &Arc<AgentState>,
    args: &Value,
    enabled: bool,
) -> Result<Value, String> {
    let name = req_str(args, "name")?;
    state.set_backend_enabled(name, enabled).await?;
    Ok(json!({
        "name": name,
        "enabled": enabled,
        "note": if enabled {
            "Starting. Its tools reach the gateway once it answers tools/list."
        } else {
            "Stopped. Every client loses its tools until it is started again."
        },
    }))
}

async fn restart_server(state: &Arc<AgentState>, args: &Value) -> Result<Value, String> {
    let name = req_str(args, "name")?;
    state.backends.restart(name).await?;
    Ok(json!({
        "name": name,
        "restarting": true,
        "note": "Check agent_get_local_server_status in a moment to see whether it came back up.",
    }))
}

async fn server_status(state: &Arc<AgentState>, args: &Value) -> Result<Value, String> {
    let wanted = opt_str(args, "name");
    let snapshot = state.backends.snapshot().await;

    if let Some(wanted) = wanted {
        if !snapshot.iter().any(|b| b.name == wanted) {
            return Err(format!(
                "No server named '{wanted}' on this Mac. Use agent_list_local_servers to see them."
            ));
        }
    }

    let servers: Vec<Value> = snapshot
        .into_iter()
        .filter(|b| wanted.is_none_or(|w| b.name == w))
        .map(|b| {
            json!({
                "name": b.name,
                "transport": b.transport,
                "enabled": b.enabled,
                "status": b.status,
                "pid": b.pid,
                "started_at": b.started_at,
                "uptime_secs": b.uptime_secs,
                "restarts": b.restarts,
                "tool_count": b.tool_count,
                "error": b.error,
            })
        })
        .collect();

    let connection = state.connection().await;
    Ok(json!({
        "agent": {
            "agent_id": connection.agent_id,
            "state": connection.state,
            "connected_since": connection.connected_since,
            "registered_tools": connection.registered_tools,
            "last_error": connection.last_error,
        },
        "count": servers.len(),
        "servers": servers,
    }))
}

async fn server_logs(state: &Arc<AgentState>, args: &Value) -> Result<Value, String> {
    let lines = opt_i64(args, "lines").unwrap_or(100).clamp(1, 500) as usize;
    // `agent` is the source the agent's own supervision and connection lines
    // are filed under; a server's lines carry its name.
    let source = opt_str(args, "name").unwrap_or("agent");

    if source != "agent"
        && !state
            .backends
            .snapshot()
            .await
            .iter()
            .any(|b| b.name == source)
    {
        return Err(format!(
            "No server named '{source}' on this Mac. Omit 'name' for the agent's own log."
        ));
    }

    let floor = match opt_str(args, "min_level") {
        Some("trace") | None => LogLevel::Trace,
        Some("debug") => LogLevel::Debug,
        Some("info") => LogLevel::Info,
        Some("warn") => LogLevel::Warn,
        Some("error") => LogLevel::Error,
        Some(other) => return Err(format!("'{other}' is not a log level")),
    };

    let all = state.logs.snapshot();
    let matching: Vec<&crate::logbuf::LogLine> = all
        .iter()
        .filter(|l| l.source == source && l.level >= floor)
        .collect();
    let skip = matching.len().saturating_sub(lines);

    Ok(json!({
        "source": source,
        "lines_dropped": state.logs.dropped(),
        "lines": matching
            .into_iter()
            .skip(skip)
            .map(|l| json!({ "ts": l.ts, "level": l.level, "message": l.message }))
            .collect::<Vec<_>>(),
    }))
}

async fn update_config(state: &Arc<AgentState>, args: &Value) -> Result<Value, String> {
    let name = req_str(args, "server")?;
    let current = state.config().await.backend(name).cloned().ok_or_else(|| {
        format!("No server named '{name}' on this Mac. Use agent_list_local_servers to see them.")
    })?;

    let updated = apply_config(args, current.clone());
    if updated == current {
        return Err("Nothing to change — supply at least one field to update".into());
    }
    updated.validate()?;

    let enabled_changed = updated.enabled != current.enabled;
    state.update_backend(name, updated.clone()).await?;
    // `update` restarts a backend under its new configuration but does not act
    // on the enabled flag, which has its own path.
    if enabled_changed {
        state.set_backend_enabled(name, updated.enabled).await?;
    }

    Ok(json!({
        "server": name,
        "updated": true,
        "enabled": updated.enabled,
        "note": "Restarted under the new configuration; its tools re-register with the gateway once it answers tools/list.",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_advertises_an_object_schema_and_a_description() {
        for tool in catalog() {
            assert_eq!(tool.input_schema["type"], "object", "{}", tool.name);
            assert!(tool.input_schema["properties"].is_object(), "{}", tool.name);
            assert!(!tool.description.is_empty(), "{}", tool.name);
        }
    }

    /// The gateway namespaces these under this Mac's agent id with a double
    /// underscore, so a control tool carrying one of its own would be
    /// indistinguishable from a tool belonging to a local server.
    #[test]
    fn no_control_tool_name_could_be_mistaken_for_a_servers() {
        for tool in catalog() {
            assert!(!tool.name.contains("__"), "{}", tool.name);
            assert!(tool.name.starts_with("agent_"), "{}", tool.name);
        }
    }

    #[test]
    fn the_catalog_and_the_dispatcher_agree() {
        for tool in catalog() {
            assert!(is_control_tool(&tool.name));
        }
        assert!(!is_control_tool("obsidian__obsidian_list_notes"));
        assert!(!is_control_tool("agent_invented"));
    }

    #[test]
    fn an_update_that_touches_nothing_leaves_the_config_alone() {
        let before = LocalBackendConfig {
            name: "obsidian".into(),
            transport: "stdio".into(),
            command: Some("uvx".into()),
            args: vec!["obsidian-mcp".into()],
            env: HashMap::from([("OBSIDIAN_TOKEN".into(), "secret".into())]),
            masked_env: vec!["OBSIDIAN_TOKEN".into()],
            ..Default::default()
        };
        assert_eq!(
            apply_config(&json!({ "server": "obsidian" }), before.clone()),
            before
        );
    }

    #[test]
    fn changing_the_arguments_keeps_the_environment() {
        let before = LocalBackendConfig {
            name: "obsidian".into(),
            transport: "stdio".into(),
            command: Some("uvx".into()),
            args: vec!["obsidian-mcp".into()],
            env: HashMap::from([("OBSIDIAN_TOKEN".into(), "secret".into())]),
            masked_env: vec!["OBSIDIAN_TOKEN".into()],
            ..Default::default()
        };
        let after = apply_config(&json!({ "args": ["obsidian-mcp", "--verbose"] }), before);
        assert_eq!(after.args, vec!["obsidian-mcp", "--verbose"]);
        assert_eq!(after.env.get("OBSIDIAN_TOKEN").unwrap(), "secret");
        assert_eq!(after.masked_env, vec!["OBSIDIAN_TOKEN"]);
    }

    /// One `masked` argument, whichever block the transport has. A caller
    /// should not have to know that stdio masks `env` and http masks `headers`.
    #[test]
    fn the_mask_list_lands_on_the_block_the_transport_has() {
        let stdio = apply_config(
            &json!({ "env": { "TOKEN": "v" }, "masked": ["TOKEN"] }),
            LocalBackendConfig {
                transport: "stdio".into(),
                ..Default::default()
            },
        );
        assert_eq!(stdio.masked_env, vec!["TOKEN"]);
        assert!(stdio.masked_headers.is_empty());

        let http = apply_config(
            &json!({ "headers": { "Authorization": "Bearer v" }, "masked": ["Authorization"] }),
            LocalBackendConfig {
                transport: "http".into(),
                ..Default::default()
            },
        );
        assert_eq!(http.masked_headers, vec!["Authorization"]);
        assert!(http.masked_env.is_empty());
    }

    /// A mask flag for a variable that is no longer there would silently hide a
    /// future variable of the same name.
    #[test]
    fn a_removed_variable_takes_its_mask_with_it() {
        let before = LocalBackendConfig {
            transport: "stdio".into(),
            env: HashMap::from([("OLD".into(), "v".into())]),
            masked_env: vec!["OLD".into()],
            ..Default::default()
        };
        let after = apply_config(&json!({ "env": { "NEW": "v" } }), before);
        assert!(after.masked_env.is_empty());
    }

    #[test]
    fn a_required_argument_says_which_one_is_missing() {
        assert!(req_str(&json!({ "server": "   " }), "server")
            .unwrap_err()
            .contains("'server'"));
    }
}
