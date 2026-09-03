# Self-Configuration Tools

The gateway aggregates other people's tools. It also exposes some of its own.

`gateway_*` configures the gateway; `agent_*` configures one Mac running the
agent. They are ordinary MCP tools in every way that matters for safety — they
appear in `tools/list`, they are classified on the same risk ladder, and policy
governs them — but they are not part of the inventory the operator assembled, so
they are left out of the counts and their calls go to the server log rather than
to the audit trail. See [They are invisible on the
gateway](#they-are-invisible-on-the-gateway). The point is that an assistant
which finds a backend unhealthy can read its logs, fix its configuration and
restart it, without anybody opening the dashboard.

- [They are invisible on the gateway](#they-are-invisible-on-the-gateway)
- [What guards them](#what-guards-them)
- [Gateway tools](#gateway-tools)
- [Agent tools](#agent-tools)
- [Worked examples](#worked-examples)
- [Turning them off](#turning-them-off)

---

## They are invisible on the gateway

The gateway is for the tools its operator put behind it. Its own plumbing is not
part of that picture, so none of it shows up where those tools are counted,
listed or traced:

| Surface | Internal tools |
|---|---|
| MCP `tools/list` and `tools/call` | **Present** — this is the whole point |
| Dashboard **Tools** page, and its counts | Absent |
| **Backends** page per-backend tool count | Absent |
| **Metrics** — tool totals, top tools, calls by risk, volume, latency | Absent |
| Audit trail, and the live event feed | Absent |
| **Usage** graph | Absent |
| Prometheus `/metrics` | Absent |

They are still resolved, still policy-evaluated and still role-gated — only the
*recording* is skipped. Nothing is hidden from the operator that they cannot get
at: every internal call is written to the server log at INFO with the caller,
the tool, the status and the duration, so `docker logs` still answers "who
reconfigured this, and when":

```
INFO Internal gateway tool call user=admin tool="gateway_register_backend" status="success" duration_ms=48.2
WARN Refused an internal gateway tool call user=ci tool="gateway_delete_policy" reason="blocked by policy"
```

The trade is deliberate and worth stating plainly: an admin action on the gateway
is **not** in the audit trail you can query from the dashboard. If you need
tamper-evident records of configuration changes, ship the server log somewhere
you keep them.

The mechanism is a `tool_registry.is_internal` flag. It is set from a fixed list
of names (`backends::classifier`), matched whole — a backend of yours that
happens to ship a tool called `agent_something` is yours, and stays visible.

---

## What guards them

Three gates, in this order.

**1. The role gate.** Every `gateway_*` tool that writes anything requires the
`owner` role, checked against the caller's claims exactly as the REST API's
`require_admin` does. A policy that allows everything still does not let a
non-owner rewrite RBAC. Three read-only tools — `gateway_get_health`,
`gateway_list_backends`, `gateway_query_audit_log` — are open to any
authenticated caller, with the same scoping their REST equivalents apply: a
non-owner sees only their own audit events, and gets no credential-bearing
configuration block at all.

**2. Policy.** Each tool carries a risk category from the same five-level ladder
the dashboard renders and the policy engine matches on:

| Category | Gateway tools | Agent tools |
|---|---|---|
| `read` | `list_policies`, `list_backends`, `test_backend_connectivity`, `get_mcp_server_status`, `get_mcp_server_logs`, `query_audit_log`, `get_health` | `list_local_servers`, `get_local_server_status`, `get_local_server_logs` |
| `execute` | `start_mcp_server`, `restart_mcp_server` | `start_local_server`, `restart_local_server` |
| `admin` | `create_policy`, `update_policy`, `set_tool_classification`, `register_backend`, `update_backend_config` | `install_mcp_server`, `update_config` |
| `destructive` | `delete_policy`, `remove_backend`, `stop_mcp_server` | `remove_mcp_server`, `stop_local_server` |

So a rule you already understand — *deny `destructive` for the `ci` role* —
governs them with no special case. To keep an application out of gateway
configuration entirely:

```jsonc
{
  "name": "No self-configuration from CI",
  "tool_pattern": "gateway_*, *__agent_*",
  "decision": "deny",
  "application_match": "ci",
  "priority": 1
}
```

Priority matters: the engine sorts ascending and the first match wins, so a
specific deny has to sit ahead of any allow that also matches.

**3. The server log.** Every internal call is written there — see
[above](#they-are-invisible-on-the-gateway). It is deliberately not in the audit
trail, which is about the traffic the operator's own tools carry.

A tool the caller cannot reach is not advertised in `tools/list` — an assistant's
context is better spent on tools it can actually call.

---

## Gateway tools

Names are matched exactly. Backend tools are always namespaced
`<backend>__<tool>` with a double underscore, so a backend that happens to be
called `gateway` produces `gateway__foo` and cannot collide with anything here.

### Policy and RBAC

| Tool | What it does |
|---|---|
| `gateway_list_policies` | Every policy in evaluation order, with the roles each is bound to. |
| `gateway_create_policy` | Write a rule: tool pattern, decision, risk categories, application, roles. Appended at the end of the order. |
| `gateway_update_policy` | Change one — including its `priority`, which is what moves a deny ahead of a broad allow. |
| `gateway_delete_policy` | Remove one. |
| `gateway_set_tool_classification` | Set a tool's risk category, which is what policy matches on and the dashboard colours. Refused for the gateway's own tools: their categories are what a policy denying them matches on. |

Policies are identified by `policy_id`, or by `name` when the name is unique.
A policy bound to no role is never evaluated; `gateway_create_policy` says so in
its result rather than leaving you to wonder why the rule does nothing.

### Backend lifecycle

| Tool | What it does |
|---|---|
| `gateway_list_backends` | Transport, health, tool count and configuration for each backend. |
| `gateway_register_backend` | Add and start a `stdio`, `streamable-http` or `sse` backend. Tools are discovered immediately. |
| `gateway_update_backend_config` | Change a backend and restart it under the new configuration. |
| `gateway_remove_backend` | Stop it, withdraw its tools, delete its configuration. |
| `gateway_test_backend_connectivity` | Probe a registered backend without changing it. |

Configuration is given as flat fields — `command`, `args`, `env`, `url`,
`headers` — rather than the nested blob the database stores. Mark credentials
with `masked`, and they are never displayed again, here or in the dashboard;
`gateway_list_backends` returns `__mcpgw_masked__` in their place, and echoing
that placeholder back into an update keeps the stored value.

`gateway_test_backend_connectivity` takes the *name* of a registered backend, not
a URL. Taking a URL would make it a request forger pointed at anything the
gateway's network can reach. On a stdio backend it asks the process that is
already running rather than respawning it, so a test is a test rather than a
restart.

Registering an `agent` backend is refused: an agent registers itself when the Mac
running it connects, and a row conjured from this side would be overwritten by
that registration. Editing one is refused for the same reason.

### Process management

| Tool | What it does |
|---|---|
| `gateway_start_mcp_server` | Enable a backend and bring it up. |
| `gateway_stop_mcp_server` | Kill it and withdraw its tools. Classifications and audit history survive. |
| `gateway_restart_mcp_server` | Restart and re-discover. For an agent backend, asks that Mac to re-register. |
| `gateway_get_mcp_server_status` | Running or not, since when, pid, start count, last error. Omit the name for all of them. |
| `gateway_get_mcp_server_logs` | The tail of a stdio backend's stderr, plus recent failed calls from the audit trail. |

A stdio backend's stderr is captured into a 500-line ring per backend and passed
through the audit redactor on the way in — backends write credentials to stderr
more often than anyone would like, and a log line is written once and read many
times. The ring outlives the process, because "why did it die" is a question you
only ask after it died.

A backend with no local process — `streamable-http`, `sse`, `agent` — has no
stderr. `gateway_get_mcp_server_logs` returns its recent failures from the audit
trail instead, which is the closest thing it has to a log.

### Audit and diagnostics

| Tool | What it does |
|---|---|
| `gateway_query_audit_log` | Search the trail by tool, backend, status, risk, decision, application and time. |
| `gateway_get_health` | Version, backend and tool counts, active policies and users, and the last 24 hours of throughput, latency and errors. |

`since` accepts `1h`, `24h`, `7d`, `30d`, or an RFC 3339 timestamp. Payloads are
stored only as hashes and are never returned.

---

## Agent tools

A smaller mirror, scoped to one Mac. The gateway namespaces them under that
machine's agent id, so a client sees `sids-macbook-pro__agent_list_local_servers`.

| Tool | What it does |
|---|---|
| `agent_list_local_servers` | Every MCP server on that Mac, with status, tool count, and the *names* of its environment. |
| `agent_install_mcp_server` | Add a server and expose it. Started and asked for its tools first; if it does not come up, nothing is written. |
| `agent_remove_mcp_server` | Stop it, withdraw its tools, delete its configuration. |
| `agent_start_local_server` / `agent_stop_local_server` | Expose it, or withdraw it. |
| `agent_restart_local_server` | Restart and re-discover. |
| `agent_get_local_server_status` | Running or not, pid, uptime, restarts, last error — plus the agent's own connection state. |
| `agent_get_local_server_logs` | Tail a server's stderr, or the agent's own log. Filterable by level. |
| `agent_update_config` | Change a server's command, arguments, environment, headers, masks, and whether it is exposed. |

Environment **values** never leave the Mac — not through these tools, not in the
registration frame, not to the dashboard. Only key names travel, plus which of
them are masked.

Deliberately absent: anything that changes the tunnel itself. A call that
repointed `gateway_url` would arrive over the connection it was about to sever,
and the answer would never get back — so the gateway address, the agent id and
the TLS setting stay in the app, where a person can see what they are doing.

---

## Worked examples

**Put a new MCP server behind the gateway.**

```jsonc
// gateway_register_backend
{
  "name": "gitea",
  "transport": "stdio",
  "command": "gitea-mcp",
  "args": ["-t", "stdio"],
  "env": { "GITEA_HOST": "https://git.example.com", "GITEA_ACCESS_TOKEN": "…" },
  "masked": ["GITEA_ACCESS_TOKEN"]
}
// → { "name": "gitea", "tools_discovered": 44, "namespace": "gitea__*" }
```

**Work out why one stopped answering.**

```jsonc
// gateway_get_mcp_server_status  { "name": "gitea" }
// → { "running": false, "starts": 3, "last_error": "Initialize failed: …" }

// gateway_get_mcp_server_logs    { "name": "gitea", "lines": 50 }
// → { "stderr": [ … ], "recent_failures": [ … ] }

// gateway_restart_mcp_server     { "name": "gitea" }
```

**Lock down a tool you have just noticed.**

```jsonc
// gateway_set_tool_classification
{ "tool_name": "gitea__delete_repo", "risk_category": "destructive" }

// gateway_create_policy
{
  "name": "No destructive tools from Claude",
  "tool_pattern": "*",
  "risk_categories": ["destructive"],
  "decision": "deny",
  "application_match": "claude",
  "roles": ["owner"]
}
// then move it ahead of the catch-all allow:
// gateway_update_policy { "policy_id": "…", "priority": 1 }
```

**Install a server on your Mac and expose it.**

```jsonc
// sids-macbook-pro__agent_install_mcp_server
{
  "name": "obsidian",
  "transport": "stdio",
  "command": "uvx",
  "args": ["mcp-obsidian"],
  "env": { "OBSIDIAN_API_KEY": "…" },
  "masked": ["OBSIDIAN_API_KEY"]
}
// → { "installed": true, "tool_count": 12, "namespace": "obsidian__*" }
```

---

## Turning them off

**On the gateway.** **Settings → Gateway tools**, or
`PATCH /api/v1/settings {"gateway_tools_enabled": false}` (owner only). It
defaults to **on**, and turning it off withdraws the namespace immediately — the
setting is read per request, not cached. With it off, `tools/list` does not offer
the tools and a call to one is answered as an unknown tool, because that is the
truth: the gateway is not offering it.

For finer control than a single switch, policy still applies — deny `gateway_*`
for one role or one application and leave it available to another. See the
example under [What guards them](#what-guards-them).

The switch governs the gateway's own namespace only. Each connected Mac decides
for itself whether to offer the tools that configure *it*.

**On a Mac.** `agent.expose_control_tools` in
`~/.mcp-gateway-agent/config.toml`, or **Settings → General → Remote control**
in the app. It defaults to **on**: "install and expose the Obsidian MCP server"
is most of the reason to have an agent at all. It is a real grant, though —
installing a server means running its command on that machine — so it is worth a
deliberate decision, and the switch is the last word for whoever owns the Mac.
Turning it off withdraws the tools from the gateway immediately; the servers
themselves keep working.

```toml
[agent]
expose_control_tools = false
```
