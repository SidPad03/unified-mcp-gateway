//! The gateway's own tools — the `gateway_*` namespace.
//!
//! Everything else the gateway exposes over MCP belongs to a backend: it was
//! discovered somewhere, and the gateway routes to it. These tools have no
//! backend. They *are* the gateway, offered to an agent as tools so that
//! configuring the thing and using it are the same kind of act — an agent that
//! finds a server unhealthy can read its logs, fix its configuration and
//! restart it without a human opening the dashboard.
//!
//! Three rules hold across the namespace, and they are what make it safe to
//! hand an agent:
//!
//! 1. **They are classified like anything else.** Every tool below carries a
//!    risk category from the same five-level ladder the dashboard renders and
//!    the policy engine matches on. Writing policy is `admin`; deleting a
//!    policy or killing a server is `destructive`. So the existing machinery —
//!    "deny destructive for the ci role", "this application may only read" —
//!    governs them with no special case.
//! 2. **Policy is not the only gate.** Almost all of them additionally require
//!    the `owner` role, checked here against the caller's claims exactly as
//!    `require_admin` does for the REST API. A policy that allows everything
//!    still does not let a non-owner rewrite RBAC.
//! 3. **They go through the same audit trail.** `api::mcp` records a
//!    `gateway_*` call the way it records any other, under the backend name
//!    `gateway`, so reconfiguring the gateway leaves the same evidence as using
//!    it.
//!
//! The names are matched exactly, never by prefix. Backend tools are namespaced
//! `<backend>__<tool>` with a double underscore, so a backend that happens to be
//! called `gateway` produces `gateway__foo` and cannot collide with anything
//! here.

use serde_json::{json, Map, Value};
use uuid::Uuid;

use crate::api::auth::Claims;
use crate::api::backends::{
    mask_secret_values, redact_backend_config, restore_masked_values, start_and_register,
    stop_and_withdraw,
};
use crate::AppState;

/// One tool in the namespace.
pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    /// From the same ladder as `tool_registry.risk_category`, so the dashboard
    /// colours it and the policy engine matches it without knowing these tools
    /// are special.
    pub risk: &'static str,
    /// Whether the caller must hold the `owner` role on top of whatever policy
    /// says. True for anything that writes, and for anything that reveals a
    /// backend's internals.
    pub owner_only: bool,
    pub input_schema: Value,
}

pub(crate) const RISK_CATEGORIES: [&str; 6] = [
    "read",
    "write",
    "execute",
    "admin",
    "destructive",
    "unclassified",
];

/// The transports an operator can register from this side. `agent` is absent on
/// purpose: an agent backend is created by the Mac that runs it when it dials
/// in, and a row conjured here would be overwritten by that registration.
const REGISTERABLE_TRANSPORTS: [&str; 3] = ["stdio", "streamable-http", "sse"];

fn schema(properties: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
    })
}

fn backend_name_prop(what: &str) -> Value {
    json!({ "type": "string", "description": what })
}

pub fn catalog() -> Vec<ToolDef> {
    vec![
        // ── Policy & RBAC ───────────────────────────────────────────────
        ToolDef {
            name: "gateway_list_policies",
            description:
                "List every RBAC policy on the gateway, in evaluation order (lowest priority \
                 number first — the first match wins), with the roles each one is bound to.",
            risk: "read",
            owner_only: true,
            input_schema: schema(json!({}), &[]),
        },
        ToolDef {
            name: "gateway_create_policy",
            description:
                "Create an RBAC policy: which tools, on which risk categories, from which \
                 applications, a role may call. The new policy is appended at the end of the \
                 evaluation order; use gateway_update_policy to give it a priority that puts \
                 it ahead of a broader rule.",
            risk: "admin",
            owner_only: true,
            input_schema: schema(
                json!({
                    "name": { "type": "string", "description": "Human-readable name for the policy." },
                    "tool_pattern": {
                        "type": "string",
                        "description": "Glob matched against the namespaced tool name, e.g. 'gitea__*' \
                                        or '*delete*'. A comma-separated list matches any of its parts."
                    },
                    "decision": { "type": "string", "enum": ["allow", "deny"] },
                    "reason": { "type": "string", "description": "Shown to the caller when this policy denies." },
                    "risk_categories": {
                        "type": "array",
                        "items": { "type": "string", "enum": RISK_CATEGORIES },
                        "description": "Restrict the rule to these categories. Empty or omitted means every category."
                    },
                    "application_match": {
                        "type": "string",
                        "description": "Glob matched against the calling application, e.g. 'claude'. Omit to match all."
                    },
                    "roles": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Role names to bind this policy to. A policy bound to no role is never evaluated."
                    }
                }),
                &["name", "tool_pattern", "decision"],
            ),
        },
        ToolDef {
            name: "gateway_update_policy",
            description:
                "Change an existing RBAC policy. Identify it by policy_id, or by name when that \
                 name is unique. Only the fields supplied are changed.",
            risk: "admin",
            owner_only: true,
            input_schema: schema(
                json!({
                    "policy_id": { "type": "string", "description": "UUID from gateway_list_policies." },
                    "name": { "type": "string", "description": "Identifies the policy when policy_id is omitted; otherwise renames it." },
                    "tool_pattern": { "type": "string" },
                    "decision": { "type": "string", "enum": ["allow", "deny"] },
                    "reason": { "type": "string" },
                    "priority": {
                        "type": "integer",
                        "description": "Evaluation order, ascending, unique across policies. A specific deny must sit ahead of a broad allow to ever fire."
                    },
                    "is_active": { "type": "boolean" },
                    "risk_categories": { "type": "array", "items": { "type": "string", "enum": RISK_CATEGORIES } },
                    "application_match": { "type": "string" },
                    "roles": { "type": "array", "items": { "type": "string" }, "description": "Replaces the policy's role bindings outright." }
                }),
                &[],
            ),
        },
        ToolDef {
            name: "gateway_delete_policy",
            description:
                "Delete an RBAC policy. Whatever it was denying becomes governed by the next \
                 matching policy, or by the role's default. Cannot be undone.",
            risk: "destructive",
            owner_only: true,
            input_schema: schema(
                json!({
                    "policy_id": { "type": "string" },
                    "name": { "type": "string", "description": "Used when policy_id is omitted; must match exactly one policy." }
                }),
                &[],
            ),
        },
        ToolDef {
            name: "gateway_set_tool_classification",
            description:
                "Set a tool's risk category. This is what the policy engine matches on and what \
                 the dashboard colours, so reclassifying a tool changes which rules govern it.",
            risk: "admin",
            owner_only: true,
            input_schema: schema(
                json!({
                    "tool_name": { "type": "string", "description": "The namespaced name, e.g. 'gitea__delete_branch'." },
                    "risk_category": { "type": "string", "enum": RISK_CATEGORIES }
                }),
                &["tool_name", "risk_category"],
            ),
        },
        // ── Backend lifecycle ───────────────────────────────────────────
        ToolDef {
            name: "gateway_list_backends",
            description:
                "Every backend the gateway aggregates, with its transport, health, tool count \
                 and configuration. Values marked secret come back as a placeholder.",
            risk: "read",
            owner_only: false,
            input_schema: schema(json!({}), &[]),
        },
        ToolDef {
            name: "gateway_register_backend",
            description:
                "Register a new MCP backend and start it. A stdio backend needs a command; an \
                 http or sse backend needs a url. Tools are discovered immediately, so the \
                 result says how many the backend advertised.",
            risk: "admin",
            owner_only: true,
            input_schema: schema(
                json!({
                    "name": { "type": "string", "description": "Unique. Becomes the tool namespace: '<name>__<tool>'." },
                    "transport": { "type": "string", "enum": REGISTERABLE_TRANSPORTS },
                    "command": { "type": "string", "description": "stdio only — the executable to run, e.g. 'npx'." },
                    "args": { "type": "array", "items": { "type": "string" }, "description": "stdio only." },
                    "env": { "type": "object", "description": "stdio only — environment for the child process." },
                    "url": { "type": "string", "description": "http/sse only — the MCP endpoint." },
                    "headers": { "type": "object", "description": "http/sse only — sent on every request, e.g. Authorization." },
                    "masked": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Keys of env or headers whose values must never be displayed again. Set this for tokens."
                    },
                    "risk_category": { "type": "string", "description": "The backend's own category, e.g. 'external-api'. Tools are classified individually." }
                }),
                &["name", "transport"],
            ),
        },
        ToolDef {
            name: "gateway_update_backend_config",
            description:
                "Change a registered backend's configuration. Only the fields supplied are \
                 changed, and the backend is restarted under the new configuration if it is \
                 enabled. An agent backend cannot be edited here — it is configured on the Mac \
                 that runs it.",
            risk: "admin",
            owner_only: true,
            input_schema: schema(
                json!({
                    "name": backend_name_prop("The backend to change."),
                    "command": { "type": "string" },
                    "args": { "type": "array", "items": { "type": "string" } },
                    "env": { "type": "object", "description": "Replaces the environment outright." },
                    "url": { "type": "string" },
                    "headers": { "type": "object", "description": "Replaces the headers outright." },
                    "masked": { "type": "array", "items": { "type": "string" } },
                    "risk_category": { "type": "string" }
                }),
                &["name"],
            ),
        },
        ToolDef {
            name: "gateway_remove_backend",
            description:
                "Remove a backend from the gateway: stop it, delete its tools from the registry, \
                 and delete its configuration. Cannot be undone. Audit history is kept.",
            risk: "destructive",
            owner_only: true,
            input_schema: schema(
                json!({ "name": backend_name_prop("The backend to remove.") }),
                &["name"],
            ),
        },
        ToolDef {
            name: "gateway_test_backend_connectivity",
            description:
                "Health-check a registered backend without changing it: confirm it answers, and \
                 report how many tools it advertises. Run it before and after a configuration \
                 change. Only registered backends can be probed.",
            risk: "read",
            owner_only: true,
            input_schema: schema(
                json!({ "name": backend_name_prop("The backend to probe.") }),
                &["name"],
            ),
        },
        // ── MCP server process management ───────────────────────────────
        ToolDef {
            name: "gateway_start_mcp_server",
            description:
                "Enable a backend and bring it up, re-registering the tools it advertises.",
            risk: "execute",
            owner_only: true,
            input_schema: schema(
                json!({ "name": backend_name_prop("The backend to start.") }),
                &["name"],
            ),
        },
        ToolDef {
            name: "gateway_stop_mcp_server",
            description: "Stop a backend and disable it: its process is killed and its tools are \
                 withdrawn from the gateway, so every client loses them until it is started \
                 again. Classifications and audit history survive.",
            risk: "destructive",
            owner_only: true,
            input_schema: schema(
                json!({ "name": backend_name_prop("The backend to stop.") }),
                &["name"],
            ),
        },
        ToolDef {
            name: "gateway_restart_mcp_server",
            description:
                "Restart a backend and re-discover its tools. For an agent backend this asks \
                 that Mac to re-send its registration.",
            risk: "execute",
            owner_only: true,
            input_schema: schema(
                json!({ "name": backend_name_prop("The backend to restart.") }),
                &["name"],
            ),
        },
        ToolDef {
            name: "gateway_get_mcp_server_status",
            description:
                "Whether a backend is running, since when, how many times it has been started, \
                 and the last error it failed with. Omit the name for every backend at once.",
            risk: "read",
            owner_only: true,
            input_schema: schema(
                json!({ "name": backend_name_prop("Omit for all backends.") }),
                &[],
            ),
        },
        ToolDef {
            name: "gateway_get_mcp_server_logs",
            description:
                "Tail what a backend has written to stderr since the gateway started it, plus \
                 its recent failed calls from the audit trail. A backend with no local process \
                 (http, sse, agent) has only the latter.",
            risk: "read",
            owner_only: true,
            input_schema: schema(
                json!({
                    "name": backend_name_prop("The backend to read."),
                    "lines": { "type": "integer", "description": "How many of the most recent lines to return. Default 100, maximum 500." }
                }),
                &["name"],
            ),
        },
        // ── Audit & diagnostics ─────────────────────────────────────────
        ToolDef {
            name: "gateway_query_audit_log",
            description:
                "Search the audit trail: every tool call the gateway routed, with its verdict, \
                 duration and outcome. Payloads are stored only as hashes and are never \
                 returned. A caller without the owner role sees only their own calls.",
            risk: "read",
            owner_only: false,
            input_schema: schema(
                json!({
                    "tool_name": { "type": "string", "description": "Substring match on the tool name." },
                    "backend": { "type": "string", "description": "Exact backend name." },
                    "status": { "type": "string", "enum": ["success", "error", "tool_error", "denied"] },
                    "risk_category": { "type": "string", "enum": RISK_CATEGORIES },
                    "policy_decision": { "type": "string", "enum": ["allow", "deny"] },
                    "application": { "type": "string", "description": "Exact application name, e.g. 'claude'." },
                    "since": { "type": "string", "description": "'1h', '24h', '7d', '30d', or an RFC 3339 timestamp. Defaults to 24h." },
                    "limit": { "type": "integer", "description": "Default 50, maximum 200." }
                }),
                &[],
            ),
        },
        ToolDef {
            name: "gateway_get_health",
            description:
                "One snapshot of the gateway: version, backend and tool counts, connected \
                 agents, policy and user counts, and the last 24 hours of throughput, latency \
                 and errors.",
            risk: "read",
            owner_only: false,
            input_schema: schema(json!({}), &[]),
        },
    ]
}

/// Look a tool up by its exact name.
pub fn find(name: &str) -> Option<ToolDef> {
    catalog().into_iter().find(|t| t.name == name)
}

fn is_owner(claims: &Claims) -> bool {
    claims.roles.iter().any(|r| r == "owner")
}

/// Whether this caller may see and call `tool` at all, before policy runs.
///
/// Used both to filter `tools/list` — advertising a tool that will always be
/// refused is just noise in an agent's context — and to refuse `tools/call`.
pub fn may_call(tool: &ToolDef, claims: &Claims) -> bool {
    !tool.owner_only || is_owner(claims)
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

fn opt_map<'a>(args: &'a Value, key: &str) -> Option<&'a Map<String, Value>> {
    args.get(key).and_then(Value::as_object)
}

fn opt_i64(args: &Value, key: &str) -> Option<i64> {
    args.get(key).and_then(Value::as_i64)
}

/// `1h` / `24h` / `7d` / `30d`, or an RFC 3339 instant.
///
/// Resolved to a timestamp here and *bound* into the query, rather than
/// interpolated as a SQL interval — this value comes from a tool argument, so
/// unlike the dashboard's fixed range selectors it is caller-controlled text.
fn since_instant(
    since: Option<&str>,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<chrono::DateTime<chrono::Utc>, String> {
    Ok(match since.unwrap_or("24h") {
        "1h" => now - chrono::Duration::hours(1),
        "24h" => now - chrono::Duration::hours(24),
        "7d" => now - chrono::Duration::days(7),
        "30d" => now - chrono::Duration::days(30),
        other => chrono::DateTime::parse_from_rfc3339(other)
            .map_err(|_| {
                format!("'since' must be '1h', '24h', '7d', '30d' or an RFC 3339 timestamp, not '{other}'")
            })?
            .with_timezone(&chrono::Utc),
    })
}

struct BackendRow {
    id: Uuid,
    name: String,
    transport: String,
    config: Value,
    enabled: bool,
}

async fn backend_by_name(state: &AppState, name: &str) -> Result<BackendRow, String> {
    let row: Option<(Uuid, String, String, Value, bool)> = sqlx::query_as(
        "SELECT backend_id, name, transport, config, is_enabled FROM backends WHERE name = $1",
    )
    .bind(name)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    row.map(|(id, name, transport, config, enabled)| BackendRow {
        id,
        name,
        transport,
        config,
        enabled,
    })
    .ok_or_else(|| format!("No backend named '{name}'. Use gateway_list_backends to see them."))
}

/// The `agent_id` an agent backend is filed under, for the registry.
fn agent_id_of(row: &BackendRow) -> String {
    row.config
        .get("agent_id")
        .and_then(Value::as_str)
        .unwrap_or(&row.name)
        .to_string()
}

/// Locate a policy by id, or by name when the name is unambiguous.
async fn policy_id_from(state: &AppState, args: &Value) -> Result<Uuid, String> {
    if let Some(raw) = opt_str(args, "policy_id") {
        return Uuid::parse_str(raw).map_err(|_| format!("'{raw}' is not a policy id"));
    }
    let name = opt_str(args, "name")
        .ok_or_else(|| "Provide 'policy_id', or 'name' when it is unique".to_string())?;
    let matches: Vec<(Uuid,)> = sqlx::query_as("SELECT policy_id FROM policies WHERE name = $1")
        .bind(name)
        .fetch_all(&state.db)
        .await
        .map_err(|e| e.to_string())?;
    match matches.len() {
        0 => Err(format!("No policy named '{name}'")),
        1 => Ok(matches[0].0),
        n => Err(format!(
            "{n} policies are named '{name}' — identify it by policy_id instead"
        )),
    }
}

async fn role_ids_for(state: &AppState, names: &[String]) -> Result<Vec<Uuid>, String> {
    let rows: Vec<(Uuid, String)> =
        sqlx::query_as("SELECT role_id, name FROM roles WHERE name = ANY($1)")
            .bind(names)
            .fetch_all(&state.db)
            .await
            .map_err(|e| e.to_string())?;
    let missing: Vec<&String> = names
        .iter()
        .filter(|n| !rows.iter().any(|(_, found)| found == *n))
        .collect();
    if !missing.is_empty() {
        return Err(format!("No such role(s): {missing:?}"));
    }
    Ok(rows.into_iter().map(|(id, _)| id).collect())
}

/// Fold the flat backend arguments into the `config` blob the rest of the
/// gateway stores, on top of whatever is already there.
///
/// `base` is the stored configuration, so an update that only changes `args`
/// keeps the environment. Secret values the caller echoed back as the mask
/// placeholder are resolved against `base` by `restore_masked_values`, which is
/// the same round trip the dashboard's editor makes.
fn build_backend_config(args: &Value, transport: &str, base: &Value) -> Result<Value, String> {
    let mut config = base.as_object().cloned().unwrap_or_default();
    let stdio = transport == "stdio";

    if stdio {
        if let Some(command) = opt_str(args, "command") {
            config.insert("command".into(), json!(command));
        }
        if let Some(list) = opt_strings(args, "args") {
            config.insert("args".into(), json!(list));
        }
        if let Some(env) = opt_map(args, "env") {
            config.insert("env".into(), Value::Object(env.clone()));
        }
    } else {
        if let Some(url) = opt_str(args, "url") {
            config.insert("url".into(), json!(url));
        }
        if let Some(headers) = opt_map(args, "headers") {
            config.insert("headers".into(), Value::Object(headers.clone()));
        }
    }

    // One `masked` list from the caller; it lands against whichever block this
    // transport actually has, so a caller never has to know the difference
    // between `masked_env` and `masked_headers`.
    if let Some(masked) = opt_strings(args, "masked") {
        let key = if stdio {
            "masked_env"
        } else {
            "masked_headers"
        };
        config.insert(key.into(), json!(masked));
    }

    let config = Value::Object(config);
    Ok(restore_masked_values(config, base))
}

// ── Dispatch ────────────────────────────────────────────────────────────

/// Run one gateway tool.
///
/// The caller (`api::mcp`) has already evaluated policy and audited the
/// attempt; this checks the role gate and does the work.
pub async fn call(
    state: &AppState,
    claims: &Claims,
    name: &str,
    args: &Value,
) -> Result<Value, String> {
    let tool = find(name).ok_or_else(|| format!("Unknown gateway tool: {name}"))?;
    if !may_call(&tool, claims) {
        return Err(format!(
            "'{name}' configures the gateway and requires the owner role"
        ));
    }

    match name {
        "gateway_list_policies" => list_policies(state).await,
        "gateway_create_policy" => create_policy(state, claims, args).await,
        "gateway_update_policy" => update_policy(state, args).await,
        "gateway_delete_policy" => delete_policy(state, args).await,
        "gateway_set_tool_classification" => set_tool_classification(state, args).await,

        "gateway_list_backends" => list_backends(state, claims).await,
        "gateway_register_backend" => register_backend(state, args).await,
        "gateway_update_backend_config" => update_backend_config(state, args).await,
        "gateway_remove_backend" => remove_backend(state, args).await,
        "gateway_test_backend_connectivity" => test_connectivity(state, args).await,

        "gateway_start_mcp_server" => start_server(state, args).await,
        "gateway_stop_mcp_server" => stop_server(state, args).await,
        "gateway_restart_mcp_server" => restart_server(state, args).await,
        "gateway_get_mcp_server_status" => server_status(state, args).await,
        "gateway_get_mcp_server_logs" => server_logs(state, args).await,

        "gateway_query_audit_log" => query_audit_log(state, claims, args).await,
        "gateway_get_health" => health(state).await,

        // Unreachable: `find` already matched against the same list.
        other => Err(format!("Unknown gateway tool: {other}")),
    }
}

// ── Policy & RBAC ───────────────────────────────────────────────────────

async fn list_policies(state: &AppState) -> Result<Value, String> {
    let rows: Vec<(
        Uuid,
        String,
        i32,
        String,
        String,
        Option<String>,
        bool,
        Option<Vec<String>>,
        Option<String>,
    )> = sqlx::query_as(
        "SELECT policy_id, name, priority, tool_pattern, decision, reason, is_active, \
                risk_categories, application_match \
         FROM policies ORDER BY priority ASC",
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    let mut policies = Vec::with_capacity(rows.len());
    for (id, name, priority, pattern, decision, reason, active, risks, application) in rows {
        let roles: Vec<(String,)> = sqlx::query_as(
            "SELECT r.name FROM roles r JOIN role_policies rp ON rp.role_id = r.role_id \
             WHERE rp.policy_id = $1 ORDER BY r.name",
        )
        .bind(id)
        .fetch_all(&state.db)
        .await
        .map_err(|e| e.to_string())?;

        policies.push(json!({
            "policy_id": id.to_string(),
            "name": name,
            "priority": priority,
            "tool_pattern": pattern,
            "decision": decision,
            "reason": reason,
            "is_active": active,
            "risk_categories": risks.unwrap_or_default(),
            "application_match": application,
            "roles": roles.into_iter().map(|(n,)| n).collect::<Vec<_>>(),
        }));
    }

    Ok(json!({
        "count": policies.len(),
        "evaluation": "ascending priority; the first matching policy decides",
        "policies": policies,
    }))
}

async fn create_policy(state: &AppState, claims: &Claims, args: &Value) -> Result<Value, String> {
    let name = req_str(args, "name")?;
    let pattern = req_str(args, "tool_pattern")?;
    let decision = req_str(args, "decision")?;
    if decision != "allow" && decision != "deny" {
        return Err("'decision' must be 'allow' or 'deny'".into());
    }
    let risks = opt_strings(args, "risk_categories").unwrap_or_default();
    for risk in &risks {
        if !RISK_CATEGORIES.contains(&risk.as_str()) {
            return Err(format!("'{risk}' is not a risk category"));
        }
    }
    let roles = opt_strings(args, "roles").unwrap_or_default();
    let role_ids = role_ids_for(state, &roles).await?;

    let created_by: Uuid = claims
        .sub
        .parse()
        .map_err(|_| "Caller has no resolvable user id".to_string())?;
    let policy_id = Uuid::new_v4();
    let now = chrono::Utc::now();

    // MAX+1 computed inside the INSERT, with a retry on the unique-priority
    // index, exactly as the REST path does — two concurrent creates must not be
    // able to claim the same slot.
    let mut attempts = 0;
    let priority = loop {
        attempts += 1;
        let result = sqlx::query_as::<_, (i32,)>(
            "INSERT INTO policies (policy_id, name, priority, conditions, decision, reason, \
                                   is_active, created_by, created_at, updated_at, tool_pattern, \
                                   risk_categories, application_match) \
             VALUES ($1, $2, (SELECT COALESCE(MAX(priority), 0) + 1 FROM policies), '{}'::jsonb, \
                     $3, $4, TRUE, $5, $6, $6, $7, $8, $9) \
             RETURNING priority",
        )
        .bind(policy_id)
        .bind(name)
        .bind(decision)
        .bind(opt_str(args, "reason"))
        .bind(created_by)
        .bind(now)
        .bind(pattern)
        .bind(&risks)
        .bind(opt_str(args, "application_match"))
        .fetch_one(&state.db)
        .await;

        match result {
            Ok((p,)) => break p,
            Err(e) if e.to_string().contains("priority") && attempts < 5 => continue,
            Err(e) => return Err(e.to_string()),
        }
    };

    for role_id in &role_ids {
        sqlx::query(
            "INSERT INTO role_policies (role_id, policy_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        )
        .bind(role_id)
        .bind(policy_id)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;
    }

    Ok(json!({
        "policy_id": policy_id.to_string(),
        "name": name,
        "priority": priority,
        "decision": decision,
        "roles": roles,
        "note": if roles.is_empty() {
            "This policy is bound to no role, so nothing evaluates it. Bind it with gateway_update_policy."
        } else {
            "Created at the end of the evaluation order. A deny only fires if it sits ahead of every allow that also matches."
        },
    }))
}

async fn update_policy(state: &AppState, args: &Value) -> Result<Value, String> {
    let policy_id = policy_id_from(state, args).await?;
    let mut changed: Vec<&str> = Vec::new();

    // `name` doubles as the lookup key, so it only renames when the caller
    // identified the policy some other way.
    if let (Some(name), Some(_)) = (opt_str(args, "name"), opt_str(args, "policy_id")) {
        sqlx::query("UPDATE policies SET name = $1, updated_at = NOW() WHERE policy_id = $2")
            .bind(name)
            .bind(policy_id)
            .execute(&state.db)
            .await
            .map_err(|e| e.to_string())?;
        changed.push("name");
    }

    if let Some(pattern) = opt_str(args, "tool_pattern") {
        sqlx::query(
            "UPDATE policies SET tool_pattern = $1, updated_at = NOW() WHERE policy_id = $2",
        )
        .bind(pattern)
        .bind(policy_id)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;
        changed.push("tool_pattern");
    }

    if let Some(decision) = opt_str(args, "decision") {
        if decision != "allow" && decision != "deny" {
            return Err("'decision' must be 'allow' or 'deny'".into());
        }
        sqlx::query("UPDATE policies SET decision = $1, updated_at = NOW() WHERE policy_id = $2")
            .bind(decision)
            .bind(policy_id)
            .execute(&state.db)
            .await
            .map_err(|e| e.to_string())?;
        changed.push("decision");
    }

    if let Some(reason) = opt_str(args, "reason") {
        sqlx::query("UPDATE policies SET reason = $1, updated_at = NOW() WHERE policy_id = $2")
            .bind(reason)
            .bind(policy_id)
            .execute(&state.db)
            .await
            .map_err(|e| e.to_string())?;
        changed.push("reason");
    }

    if let Some(priority) = opt_i64(args, "priority") {
        let priority = i32::try_from(priority).map_err(|_| "'priority' is out of range")?;
        let conflict: Option<(Uuid,)> = sqlx::query_as(
            "SELECT policy_id FROM policies WHERE priority = $1 AND policy_id != $2",
        )
        .bind(priority)
        .bind(policy_id)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| e.to_string())?;
        if conflict.is_some() {
            return Err(format!(
                "Priority {priority} is already taken. Priorities are unique; \
                 gateway_list_policies shows which are in use."
            ));
        }
        sqlx::query("UPDATE policies SET priority = $1, updated_at = NOW() WHERE policy_id = $2")
            .bind(priority)
            .bind(policy_id)
            .execute(&state.db)
            .await
            .map_err(|e| e.to_string())?;
        changed.push("priority");
    }

    if let Some(active) = args.get("is_active").and_then(Value::as_bool) {
        sqlx::query("UPDATE policies SET is_active = $1, updated_at = NOW() WHERE policy_id = $2")
            .bind(active)
            .bind(policy_id)
            .execute(&state.db)
            .await
            .map_err(|e| e.to_string())?;
        changed.push("is_active");
    }

    if let Some(risks) = opt_strings(args, "risk_categories") {
        for risk in &risks {
            if !RISK_CATEGORIES.contains(&risk.as_str()) {
                return Err(format!("'{risk}' is not a risk category"));
            }
        }
        sqlx::query(
            "UPDATE policies SET risk_categories = $1, updated_at = NOW() WHERE policy_id = $2",
        )
        .bind(&risks)
        .bind(policy_id)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;
        changed.push("risk_categories");
    }

    if let Some(application) = opt_str(args, "application_match") {
        sqlx::query(
            "UPDATE policies SET application_match = $1, updated_at = NOW() WHERE policy_id = $2",
        )
        .bind(application)
        .bind(policy_id)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;
        changed.push("application_match");
    }

    if let Some(roles) = opt_strings(args, "roles") {
        let role_ids = role_ids_for(state, &roles).await?;
        sqlx::query("DELETE FROM role_policies WHERE policy_id = $1")
            .bind(policy_id)
            .execute(&state.db)
            .await
            .map_err(|e| e.to_string())?;
        for role_id in role_ids {
            sqlx::query("INSERT INTO role_policies (role_id, policy_id) VALUES ($1, $2) ON CONFLICT DO NOTHING")
                .bind(role_id)
                .bind(policy_id)
                .execute(&state.db)
                .await
                .map_err(|e| e.to_string())?;
        }
        changed.push("roles");
    }

    if changed.is_empty() {
        return Err("Nothing to change — supply at least one field to update".into());
    }

    Ok(json!({
        "policy_id": policy_id.to_string(),
        "updated": changed,
    }))
}

async fn delete_policy(state: &AppState, args: &Value) -> Result<Value, String> {
    let policy_id = policy_id_from(state, args).await?;
    let deleted = sqlx::query("DELETE FROM policies WHERE policy_id = $1")
        .bind(policy_id)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;
    if deleted.rows_affected() == 0 {
        return Err("That policy no longer exists".into());
    }
    Ok(json!({ "policy_id": policy_id.to_string(), "deleted": true }))
}

async fn set_tool_classification(state: &AppState, args: &Value) -> Result<Value, String> {
    let tool_name = req_str(args, "tool_name")?;
    let risk = req_str(args, "risk_category")?;
    if !RISK_CATEGORIES.contains(&risk) {
        return Err(format!(
            "'{risk}' is not a risk category. Use one of {RISK_CATEGORIES:?}"
        ));
    }

    let row: Option<(Option<String>, bool)> =
        sqlx::query_as("SELECT risk_category, is_internal FROM tool_registry WHERE tool_name = $1")
            .bind(tool_name)
            .fetch_optional(&state.db)
            .await
            .map_err(|e| e.to_string())?;
    let (previous, is_internal) = row.ok_or_else(|| {
        format!(
            "No tool named '{tool_name}' is registered. Names are namespaced '<backend>__<tool>'."
        )
    })?;

    // The control tools' categories are what a "deny destructive" policy
    // matches on to keep them out of the wrong hands. Letting one of those
    // tools reclassify itself down to `read` would undo that in a single call.
    if is_internal {
        return Err(format!(
            "'{tool_name}' is one of the gateway's own tools. Their classifications are fixed, because a policy denying them is what keeps them governed."
        ));
    }

    sqlx::query("UPDATE tool_registry SET risk_category = $1 WHERE tool_name = $2")
        .bind(risk)
        .bind(tool_name)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;

    Ok(json!({
        "tool_name": tool_name,
        "was": previous.unwrap_or_else(|| "unclassified".into()),
        "now": risk,
        "note": "Policies matching on risk category apply to this tool from the next call onward.",
    }))
}

// ── Backend lifecycle ───────────────────────────────────────────────────

async fn list_backends(state: &AppState, claims: &Claims) -> Result<Value, String> {
    let owner = is_owner(claims);
    let rows: Vec<(
        Uuid,
        String,
        String,
        Value,
        Option<String>,
        bool,
        String,
        Option<chrono::DateTime<chrono::Utc>>,
    )> = sqlx::query_as(
        "SELECT backend_id, name, transport, config, risk_category, is_enabled, \
                    health_status, last_health_check \
             FROM backends ORDER BY name",
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    let mut backends = Vec::with_capacity(rows.len());
    for (id, name, transport, config, risk, enabled, health, checked) in rows {
        let (tool_count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM tool_registry WHERE backend_id = $1 AND is_internal = FALSE",
        )
        .bind(id)
        .fetch_one(&state.db)
        .await
        .map_err(|e| e.to_string())?;

        // Same rule as `GET /backends`: an owner sees the configuration with
        // secret values replaced by a placeholder, everyone else gets no
        // credential-bearing block at all.
        let config = if owner {
            mask_secret_values(config)
        } else {
            redact_backend_config(config)
        };

        backends.push(json!({
            "name": name,
            "transport": transport,
            "is_enabled": enabled,
            "health_status": health,
            "last_health_check": checked.map(|t| t.to_rfc3339()),
            "risk_category": risk,
            "tool_count": tool_count,
            "config": config,
            "editable": transport != "agent",
        }));
    }

    Ok(json!({ "count": backends.len(), "backends": backends }))
}

async fn register_backend(state: &AppState, args: &Value) -> Result<Value, String> {
    let name = req_str(args, "name")?;
    let transport = req_str(args, "transport")?;

    if transport == "agent" {
        return Err(
            "An agent backend registers itself when the Mac running the agent connects. \
             Install the macOS app and point it at this gateway instead."
                .into(),
        );
    }
    if !REGISTERABLE_TRANSPORTS.contains(&transport) {
        return Err(format!(
            "'transport' must be one of {REGISTERABLE_TRANSPORTS:?}"
        ));
    }
    crate::api::backends::validate_backend_name(name)?;
    if transport == "stdio" && opt_str(args, "command").is_none() {
        return Err("A stdio backend needs a 'command'".into());
    }
    if transport != "stdio" && opt_str(args, "url").is_none() {
        return Err(format!("A {transport} backend needs a 'url'"));
    }

    let config = build_backend_config(args, transport, &Value::Null)?;
    let backend_id = Uuid::new_v4();

    sqlx::query(
        "INSERT INTO backends (backend_id, name, transport, config, risk_category, is_enabled, health_status) \
         VALUES ($1, $2, $3, $4, $5, TRUE, 'idle')",
    )
    .bind(backend_id)
    .bind(name)
    .bind(transport)
    .bind(&config)
    .bind(opt_str(args, "risk_category"))
    .execute(&state.db)
    .await
    .map_err(|e| {
        if e.to_string().contains("duplicate") {
            format!("A backend named '{name}' already exists")
        } else {
            e.to_string()
        }
    })?;

    match start_and_register(state, backend_id, name, transport, &config).await {
        Ok(discovered) => Ok(json!({
            "name": name,
            "transport": transport,
            "health_status": "healthy",
            "tools_discovered": discovered.unwrap_or(0),
            "namespace": format!("{name}__*"),
        })),
        // The row is kept: a backend that failed to start is a configuration to
        // fix, not one to throw away, and deleting it here would lose whatever
        // the caller just typed.
        Err(e) => Ok(json!({
            "name": name,
            "transport": transport,
            "health_status": "unhealthy",
            "tools_discovered": 0,
            "error": e,
            "note": "Registered but not running. Fix it with gateway_update_backend_config, then gateway_restart_mcp_server.",
        })),
    }
}

async fn update_backend_config(state: &AppState, args: &Value) -> Result<Value, String> {
    let name = req_str(args, "name")?;
    let backend = backend_by_name(state, name).await?;

    if backend.transport == "agent" {
        return Err(
            "An agent backend is configured in the macOS app on the machine that runs it. \
             The gateway can start and stop it, but its configuration is not editable here."
                .into(),
        );
    }

    let config = build_backend_config(args, &backend.transport, &backend.config)?;
    let risk = opt_str(args, "risk_category");
    if config == backend.config && risk.is_none() {
        return Err("Nothing to change — supply at least one field to update".into());
    }

    sqlx::query("UPDATE backends SET config = $1 WHERE backend_id = $2")
        .bind(&config)
        .bind(backend.id)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;

    if let Some(risk) = risk {
        sqlx::query("UPDATE backends SET risk_category = $1 WHERE backend_id = $2")
            .bind(risk)
            .bind(backend.id)
            .execute(&state.db)
            .await
            .map_err(|e| e.to_string())?;
    }

    if !backend.enabled {
        return Ok(json!({
            "name": name,
            "updated": true,
            "restarted": false,
            "note": "The backend is disabled, so the new configuration takes effect when it is started.",
        }));
    }

    match start_and_register(state, backend.id, name, &backend.transport, &config).await {
        Ok(discovered) => Ok(json!({
            "name": name,
            "updated": true,
            "restarted": true,
            "health_status": "healthy",
            "tools_discovered": discovered.unwrap_or(0),
        })),
        Err(e) => Ok(json!({
            "name": name,
            "updated": true,
            "restarted": true,
            "health_status": "unhealthy",
            "error": e,
        })),
    }
}

async fn remove_backend(state: &AppState, args: &Value) -> Result<Value, String> {
    let name = req_str(args, "name")?;
    let backend = backend_by_name(state, name).await?;

    state.backend_manager.stop_backend(&backend.id).await;

    let removed_tools = sqlx::query("DELETE FROM tool_registry WHERE backend_id = $1")
        .bind(backend.id)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?
        .rows_affected();

    sqlx::query("DELETE FROM backends WHERE backend_id = $1")
        .bind(backend.id)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;

    Ok(json!({
        "name": name,
        "removed": true,
        "tools_withdrawn": removed_tools,
        "note": if backend.transport == "agent" {
            "The Mac running this agent will register it again the next time it connects."
        } else {
            "Audit history for this backend is kept."
        },
    }))
}

async fn test_connectivity(state: &AppState, args: &Value) -> Result<Value, String> {
    let name = req_str(args, "name")?;
    let backend = backend_by_name(state, name).await?;
    let started = std::time::Instant::now();

    // Deliberately probes only *registered* backends. Taking a URL as an
    // argument would turn this into a request forger pointed at anything the
    // gateway's network can reach.
    let outcome = match backend.transport.as_str() {
        // Re-running discovery on a stdio backend would kill and respawn the
        // process, which is a restart, not a test. Ask the process that is
        // already up instead.
        "stdio" => state.backend_manager.probe(&backend.id).await,
        // Not `discover_http_tools`: that hands its session to the backend's
        // calls and ends the one they were using.
        "streamable-http" => {
            crate::backends::BackendManager::probe_http_tools(name, &backend.config).await
        }
        "sse" => crate::backends::BackendManager::discover_sse_tools(name, &backend.config)
            .await
            .map(|tools| tools.len()),
        "agent" => {
            let agent_id = agent_id_of(&backend);
            if state.agent_registry.is_connected(&agent_id).await {
                let (count,): (i64,) =
                    sqlx::query_as(
                "SELECT COUNT(*) FROM tool_registry WHERE backend_id = $1 AND is_internal = FALSE",
            )
                        .bind(backend.id)
                        .fetch_one(&state.db)
                        .await
                        .map_err(|e| e.to_string())?;
                Ok(count as usize)
            } else {
                Err(format!("Agent '{agent_id}' is not connected"))
            }
        }
        other => Err(format!("Unsupported transport: {other}")),
    };

    let took_ms = started.elapsed().as_millis() as u64;
    match outcome {
        Ok(tool_count) => Ok(json!({
            "name": name,
            "transport": backend.transport,
            "reachable": true,
            "tool_count": tool_count,
            "took_ms": took_ms,
        })),
        Err(e) => Ok(json!({
            "name": name,
            "transport": backend.transport,
            "reachable": false,
            "error": e,
            "took_ms": took_ms,
        })),
    }
}

// ── Process management ──────────────────────────────────────────────────

async fn start_server(state: &AppState, args: &Value) -> Result<Value, String> {
    let name = req_str(args, "name")?;
    let backend = backend_by_name(state, name).await?;

    sqlx::query("UPDATE backends SET is_enabled = TRUE WHERE backend_id = $1")
        .bind(backend.id)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;

    match start_and_register(state, backend.id, name, &backend.transport, &backend.config).await {
        Ok(Some(count)) => Ok(json!({
            "name": name,
            "running": true,
            "tools_registered": count,
        })),
        Ok(None) => Ok(json!({
            "name": name,
            "running": state.agent_registry.is_connected(&agent_id_of(&backend)).await,
            "note": "Enabled. An agent backend comes up when the Mac running it connects; the gateway cannot start it from here.",
        })),
        Err(e) => Err(format!("'{name}' did not start: {e}")),
    }
}

async fn stop_server(state: &AppState, args: &Value) -> Result<Value, String> {
    let name = req_str(args, "name")?;
    let backend = backend_by_name(state, name).await?;

    sqlx::query("UPDATE backends SET is_enabled = FALSE WHERE backend_id = $1")
        .bind(backend.id)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;

    // Counted before the withdrawal, and only the enabled ones: a tool somebody
    // had already switched off individually was not being served, so reporting
    // it as withdrawn here would overstate what just changed.
    let (withdrawn,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM tool_registry          WHERE backend_id = $1 AND is_enabled = TRUE AND is_internal = FALSE",
    )
    .bind(backend.id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    stop_and_withdraw(state, backend.id).await;

    Ok(json!({
        "name": name,
        "running": false,
        "tools_withdrawn": withdrawn,
        "note": "Every client loses these tools until the backend is started again.",
    }))
}

async fn restart_server(state: &AppState, args: &Value) -> Result<Value, String> {
    let name = req_str(args, "name")?;
    let backend = backend_by_name(state, name).await?;

    if !backend.enabled {
        return Err(format!(
            "'{name}' is disabled. Use gateway_start_mcp_server to bring it up."
        ));
    }

    if backend.transport == "agent" {
        let agent_id = agent_id_of(&backend);
        state
            .agent_registry
            .request_resync(&agent_id)
            .await
            .map_err(|e| format!("Could not reach agent '{agent_id}': {e}"))?;
        return Ok(json!({
            "name": name,
            "restarted": true,
            "note": "Asked the agent to re-send its registration. Its tool list refreshes within a few seconds.",
        }));
    }

    // `spawn_backend` stops any existing process first, so this is the restart.
    let discovered =
        start_and_register(state, backend.id, name, &backend.transport, &backend.config)
            .await
            .map_err(|e| format!("'{name}' did not come back up: {e}"))?;

    Ok(json!({
        "name": name,
        "restarted": true,
        "tools_registered": discovered.unwrap_or(0),
    }))
}

async fn server_status(state: &AppState, args: &Value) -> Result<Value, String> {
    let wanted = opt_str(args, "name");
    let rows: Vec<(
        Uuid,
        String,
        String,
        Value,
        bool,
        String,
        Option<chrono::DateTime<chrono::Utc>>,
    )> = sqlx::query_as(
        "SELECT backend_id, name, transport, config, is_enabled, health_status, last_health_check \
             FROM backends ORDER BY name",
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    let mut servers = Vec::new();
    let mut matched = false;
    for (id, name, transport, config, enabled, health, checked) in rows {
        if let Some(wanted) = wanted {
            if name != wanted {
                continue;
            }
        }
        matched = true;

        let (tool_count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM tool_registry WHERE backend_id = $1 AND is_internal = FALSE",
        )
        .bind(id)
        .fetch_one(&state.db)
        .await
        .map_err(|e| e.to_string())?;

        let mut entry = json!({
            "name": name,
            "transport": transport,
            "is_enabled": enabled,
            "health_status": health,
            "last_health_check": checked.map(|t| t.to_rfc3339()),
            "tool_count": tool_count,
        });

        match transport.as_str() {
            "stdio" => {
                let running = state.backend_manager.is_running(&id).await;
                entry["running"] = json!(running);
                // Zero lines: this is the status, the logs have their own tool.
                if let Some(record) = state.backend_manager.record(&id, 0).await {
                    entry["pid"] = json!(record.pid);
                    entry["started_at"] = json!(record.started_at.map(|t| t.to_rfc3339()));
                    entry["uptime_secs"] = json!(record
                        .started_at
                        .filter(|_| running)
                        .map(|t| (chrono::Utc::now() - t).num_seconds().max(0)));
                    entry["starts"] = json!(record.starts);
                    entry["last_error"] = json!(record.last_error);
                }
            }
            "agent" => {
                let agent_id = config
                    .get("agent_id")
                    .and_then(Value::as_str)
                    .unwrap_or(&name)
                    .to_string();
                entry["running"] = json!(state.agent_registry.is_connected(&agent_id).await);
                entry["agent_id"] = json!(agent_id);
                entry["note"] = json!(
                    "Runs on its own machine; the gateway sees only whether it is connected."
                );
            }
            _ => {
                // No process on this side, so "running" would be a guess. The
                // health row is what the last probe actually found.
                entry["note"] = json!(
                    "Remote endpoint — health reflects the last successful discovery or call."
                );
            }
        }

        servers.push(entry);
    }

    if let Some(wanted) = wanted {
        if !matched {
            return Err(format!("No backend named '{wanted}'"));
        }
    }

    Ok(json!({ "count": servers.len(), "servers": servers }))
}

async fn server_logs(state: &AppState, args: &Value) -> Result<Value, String> {
    let name = req_str(args, "name")?;
    let lines = opt_i64(args, "lines").unwrap_or(100).clamp(1, 500) as usize;
    let backend = backend_by_name(state, name).await?;

    let mut out = json!({
        "name": name,
        "transport": backend.transport,
    });

    if backend.transport == "stdio" {
        match state.backend_manager.record(&backend.id, lines).await {
            Some(record) => {
                out["last_error"] = json!(record.last_error);
                out["lines_dropped"] = json!(record.log_dropped);
                out["stderr"] = serde_json::to_value(&record.log).unwrap_or(json!([]));
                if record.log.is_empty() {
                    out["note"] =
                        json!("The process has written nothing to stderr since it was started.");
                }
            }
            None => {
                out["stderr"] = json!([]);
                out["note"] = json!(
                    "The gateway has not started this backend since it last restarted, so there \
                     is no process output to show."
                );
            }
        }
    } else {
        out["stderr"] = json!([]);
        out["note"] = json!(
            "This backend has no local process — it runs elsewhere. Its recent failures are below."
        );
    }

    // Failed calls are the closest thing to logs a remote backend has, and are
    // worth having next to a local backend's stderr too.
    let failures: Vec<(
        chrono::DateTime<chrono::Utc>,
        String,
        String,
        Option<String>,
    )> = sqlx::query_as(
        "SELECT timestamp, tool_name, status, error_message FROM audit_events \
             WHERE backend_name = $1 AND status IN ('error', 'tool_error', 'denied') \
             ORDER BY timestamp DESC LIMIT $2",
    )
    .bind(name)
    .bind(lines.min(50) as i64)
    .fetch_all(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    out["recent_failures"] = json!(failures
        .into_iter()
        .map(|(ts, tool, status, error)| json!({
            "timestamp": ts.to_rfc3339(),
            "tool_name": tool,
            "status": status,
            "error": error,
        }))
        .collect::<Vec<_>>());

    Ok(out)
}

// ── Audit & diagnostics ─────────────────────────────────────────────────

async fn query_audit_log(state: &AppState, claims: &Claims, args: &Value) -> Result<Value, String> {
    let limit = opt_i64(args, "limit").unwrap_or(50).clamp(1, 200);
    let since = since_instant(opt_str(args, "since"), chrono::Utc::now())?;

    // A non-owner sees their own calls and nothing else — the same scoping
    // `GET /audit` applies, enforced here rather than trusted from an argument.
    let own_only = if is_owner(claims) {
        None
    } else {
        Some(
            claims
                .sub
                .parse::<Uuid>()
                .map_err(|_| "Caller has no resolvable user id".to_string())?,
        )
    };

    let mut qb = sqlx::QueryBuilder::<sqlx::Postgres>::new(
        "SELECT timestamp, tool_name, backend_name, risk_category, status, policy_decision, \
                duration_ms, error_message, application \
         FROM audit_events WHERE timestamp >= ",
    );
    qb.push_bind(since);

    if let Some(user_id) = own_only {
        qb.push(" AND user_id = ");
        qb.push_bind(user_id);
    }
    if let Some(tool) = opt_str(args, "tool_name") {
        qb.push(" AND tool_name ILIKE '%' || ");
        qb.push_bind(tool.to_string());
        qb.push(" || '%'");
    }
    if let Some(backend) = opt_str(args, "backend") {
        qb.push(" AND backend_name = ");
        qb.push_bind(backend.to_string());
    }
    if let Some(status) = opt_str(args, "status") {
        qb.push(" AND status = ");
        qb.push_bind(status.to_string());
    }
    if let Some(risk) = opt_str(args, "risk_category") {
        qb.push(" AND risk_category = ");
        qb.push_bind(risk.to_string());
    }
    if let Some(decision) = opt_str(args, "policy_decision") {
        qb.push(" AND policy_decision = ");
        qb.push_bind(decision.to_string());
    }
    if let Some(application) = opt_str(args, "application") {
        qb.push(" AND application = ");
        qb.push_bind(application.to_string());
    }
    qb.push(" ORDER BY timestamp DESC LIMIT ");
    qb.push_bind(limit);

    let rows: Vec<(
        chrono::DateTime<chrono::Utc>,
        String,
        String,
        Option<String>,
        String,
        Option<String>,
        Option<f64>,
        Option<String>,
        Option<String>,
    )> = qb
        .build_query_as()
        .fetch_all(&state.db)
        .await
        .map_err(|e| e.to_string())?;

    let events: Vec<Value> = rows
        .into_iter()
        .map(
            |(ts, tool, backend, risk, status, decision, duration, error, application)| {
                json!({
                    "timestamp": ts.to_rfc3339(),
                    "tool_name": tool,
                    "backend_name": backend,
                    "risk_category": risk,
                    "status": status,
                    "policy_decision": decision,
                    "duration_ms": duration,
                    "error_message": error,
                    "application": application,
                })
            },
        )
        .collect();

    Ok(json!({
        "since": since.to_rfc3339(),
        "scope": if own_only.is_some() { "own calls only" } else { "all users" },
        "count": events.len(),
        "truncated": events.len() as i64 == limit,
        "events": events,
    }))
}

/// The gateway's own snapshot of itself.
///
/// The tool counts carry `is_internal = FALSE` for the same reason every other
/// operator-facing count does: the `gateway_*` and `agent_*` control tools are
/// this product's plumbing, not the inventory somebody put behind it. Without
/// the filter this was the one gateway-wide tool total that disagreed with the
/// Metrics page, by nine per connected Mac.
async fn health(state: &AppState) -> Result<Value, String> {
    let row: (i64, i64, i64, i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT \
           (SELECT COUNT(*) FROM backends), \
           (SELECT COUNT(*) FROM backends WHERE is_enabled = TRUE), \
           (SELECT COUNT(*) FROM backends WHERE is_enabled = TRUE AND health_status = 'healthy'), \
           (SELECT COUNT(*) FROM tool_registry WHERE is_internal = FALSE), \
           (SELECT COUNT(*) FROM tool_registry WHERE is_enabled = TRUE AND is_internal = FALSE), \
           (SELECT COUNT(*) FROM policies WHERE is_active = TRUE), \
           (SELECT COUNT(*) FROM users WHERE is_active = TRUE), \
           (SELECT COUNT(*) FROM audit_events WHERE timestamp > NOW() - INTERVAL '24 hours')",
    )
    .fetch_one(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    let (
        backends_total,
        backends_enabled,
        backends_healthy,
        tools_total,
        tools_enabled,
        policies_active,
        users_active,
        calls_24h,
    ) = row;

    let (errors_24h,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM audit_events \
         WHERE status IN ('error', 'tool_error') AND timestamp > NOW() - INTERVAL '24 hours'",
    )
    .fetch_one(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    let (avg_latency,): (Option<f64>,) = sqlx::query_as(
        "SELECT AVG(duration_ms) FROM audit_events \
         WHERE duration_ms IS NOT NULL AND timestamp > NOW() - INTERVAL '24 hours'",
    )
    .fetch_one(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    let unhealthy: Vec<(String, String)> = sqlx::query_as(
        "SELECT name, health_status FROM backends \
         WHERE is_enabled = TRUE AND health_status NOT IN ('healthy', 'idle') ORDER BY name",
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| e.to_string())?;

    Ok(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "database": "reachable",
        "backends": {
            "total": backends_total,
            "enabled": backends_enabled,
            "healthy": backends_healthy,
            "needs_attention": unhealthy
                .into_iter()
                .map(|(name, status)| json!({ "name": name, "health_status": status }))
                .collect::<Vec<_>>(),
        },
        "tools": { "total": tools_total, "enabled": tools_enabled },
        "policies_active": policies_active,
        "users_active": users_active,
        "last_24h": {
            "calls": calls_24h,
            "errors": errors_24h,
            "error_rate": if calls_24h > 0 { errors_24h as f64 / calls_24h as f64 } else { 0.0 },
            "avg_latency_ms": avg_latency.unwrap_or(0.0),
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims(roles: &[&str]) -> Claims {
        Claims {
            sub: "6f1b9f5a-0000-4000-8000-000000000001".into(),
            username: "someone".into(),
            roles: roles.iter().map(|r| r.to_string()).collect(),
            exp: 0,
            iat: 0,
            application: None,
        }
    }

    #[test]
    fn every_tool_is_classified_on_the_same_ladder_as_a_backend_tool() {
        for tool in catalog() {
            assert!(
                RISK_CATEGORIES.contains(&tool.risk),
                "{} carries '{}', which the policy engine cannot match",
                tool.name,
                tool.risk
            );
        }
    }

    /// The guardrail: nothing that rewrites policy, edits a backend or kills a
    /// server may be classified below `admin`, or a "deny destructive" policy
    /// would leave the gateway's own configuration wide open.
    #[test]
    fn anything_that_writes_is_admin_or_destructive() {
        let writers = [
            "gateway_create_policy",
            "gateway_update_policy",
            "gateway_delete_policy",
            "gateway_set_tool_classification",
            "gateway_register_backend",
            "gateway_update_backend_config",
            "gateway_remove_backend",
            "gateway_stop_mcp_server",
        ];
        for name in writers {
            let tool = find(name).unwrap_or_else(|| panic!("{name} is missing from the catalog"));
            assert!(
                matches!(tool.risk, "admin" | "destructive"),
                "{name} is classified '{}'",
                tool.risk
            );
            assert!(tool.owner_only, "{name} must also require the owner role");
        }
    }

    /// Starting and restarting are `execute` rather than `admin`: they change
    /// no configuration. They still require the owner role.
    #[test]
    fn lifecycle_tools_are_execute_and_owner_only() {
        for name in ["gateway_start_mcp_server", "gateway_restart_mcp_server"] {
            let tool = find(name).unwrap();
            assert_eq!(tool.risk, "execute");
            assert!(tool.owner_only);
        }
    }

    /// A non-owner may still ask what is going on — with the audit trail scoped
    /// to their own calls, and backend configuration redacted, by the handlers.
    #[test]
    fn the_read_only_view_is_open_to_any_authenticated_caller() {
        for name in [
            "gateway_get_health",
            "gateway_list_backends",
            "gateway_query_audit_log",
        ] {
            let tool = find(name).unwrap();
            assert_eq!(tool.risk, "read");
            assert!(may_call(&tool, &claims(&["viewer"])), "{name}");
        }
    }

    #[test]
    fn a_non_owner_cannot_reach_the_configuration_tools() {
        let viewer = claims(&["viewer"]);
        let owner = claims(&["owner"]);
        let tool = find("gateway_create_policy").unwrap();
        assert!(!may_call(&tool, &viewer));
        assert!(may_call(&tool, &owner));
    }

    /// Names are matched exactly. A backend called `gateway` namespaces its
    /// tools `gateway__x` — prefix matching would have swallowed those.
    #[test]
    fn a_backend_called_gateway_does_not_collide_with_the_namespace() {
        assert!(find("gateway_get_health").is_some());
        // A backend named `gateway` namespaces its tools with a double
        // underscore; prefix matching would have swallowed those.
        assert!(find("gateway__get_health").is_none());
        assert!(find("gateway_").is_none());
        assert!(find("gateway_something_invented").is_none());
        for tool in catalog() {
            assert!(
                !tool.name.contains("__"),
                "{} would be ambiguous with a backend tool",
                tool.name
            );
        }
    }

    #[test]
    fn every_tool_advertises_an_object_schema() {
        for tool in catalog() {
            assert_eq!(tool.input_schema["type"], "object", "{}", tool.name);
            assert!(tool.input_schema["properties"].is_object(), "{}", tool.name);
            assert!(!tool.description.is_empty(), "{}", tool.name);
        }
    }

    #[test]
    fn since_accepts_the_shorthands_and_rfc3339_and_nothing_else() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-03-01T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let at = |arg: Option<&str>| since_instant(arg, now).map(|t| t.to_rfc3339());

        assert_eq!(at(Some("1h")).unwrap(), "2026-03-01T11:00:00+00:00");
        assert_eq!(at(Some("24h")).unwrap(), "2026-02-28T12:00:00+00:00");
        assert_eq!(at(Some("7d")).unwrap(), "2026-02-22T12:00:00+00:00");
        assert_eq!(at(Some("30d")).unwrap(), "2026-01-30T12:00:00+00:00");
        // Omitted means the last day.
        assert_eq!(at(None).unwrap(), at(Some("24h")).unwrap());
        assert_eq!(
            at(Some("2026-01-02T03:04:05Z")).unwrap(),
            "2026-01-02T03:04:05+00:00"
        );

        assert!(at(Some("yesterday")).is_err());
        // The value is bound into the query rather than interpolated, but
        // rejecting it at the parse is the first line of defence and is worth
        // pinning: this argument is caller-controlled text, unlike the fixed
        // range selectors the dashboard sends.
        assert!(at(Some("1' OR '1'='1")).is_err());
    }

    #[test]
    fn a_masked_secret_survives_an_edit_that_does_not_retype_it() {
        let stored = json!({
            "command": "gitea-mcp",
            "env": { "GITEA_TOKEN": "the-real-token", "GITEA_URL": "http://gitea.local" },
            "masked_env": ["GITEA_TOKEN"],
        });
        // What an agent would get back from `gateway_list_backends`, echoed
        // into an update that only means to change the URL.
        let args = json!({
            "env": { "GITEA_TOKEN": "__mcpgw_masked__", "GITEA_URL": "http://gitea.internal" },
            "masked": ["GITEA_TOKEN"],
        });
        let updated = build_backend_config(&args, "stdio", &stored).unwrap();
        assert_eq!(updated["env"]["GITEA_TOKEN"], "the-real-token");
        assert_eq!(updated["env"]["GITEA_URL"], "http://gitea.internal");
    }

    #[test]
    fn an_update_that_touches_nothing_leaves_the_config_alone() {
        let stored = json!({ "command": "gitea-mcp", "args": ["serve"] });
        let unchanged =
            build_backend_config(&json!({ "name": "gitea" }), "stdio", &stored).unwrap();
        assert_eq!(unchanged, stored);
    }

    #[test]
    fn the_mask_list_lands_on_the_block_the_transport_actually_has() {
        let stdio = build_backend_config(
            &json!({ "command": "x", "env": { "T": "v" }, "masked": ["T"] }),
            "stdio",
            &Value::Null,
        )
        .unwrap();
        assert_eq!(stdio["masked_env"], json!(["T"]));
        assert!(stdio.get("masked_headers").is_none());

        let http = build_backend_config(
            &json!({ "url": "http://x/mcp", "headers": { "Authorization": "Bearer v" }, "masked": ["Authorization"] }),
            "streamable-http",
            &Value::Null,
        )
        .unwrap();
        assert_eq!(http["masked_headers"], json!(["Authorization"]));
        assert!(http.get("masked_env").is_none());
    }

    #[test]
    fn a_required_argument_says_which_one_is_missing() {
        let err = req_str(&json!({ "name": "  " }), "name").unwrap_err();
        assert!(err.contains("'name'"), "{err}");
    }
}
