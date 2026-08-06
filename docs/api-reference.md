# API Reference

All REST endpoints live under `/api/v1`. Every request must carry an
`Authorization` header with either a JWT (from `/auth/login`) or an API key.

```
Authorization: Bearer <jwt_or_mcpgw_api_key>
```

Endpoints marked **owner** require the `owner` role. Endpoints marked **scoped**
are available to any authenticated user but return only that user's own rows
unless the caller is an owner.

---

## Authentication

### `POST /api/v1/auth/login`

Exchange credentials for a JWT.

```json
{ "username": "admin", "password": "your-password" }
```

Returns the token, its expiry, and the user record. If the account still owes a
first-login password change, the response carries `must_change_password: true`
and the token is only accepted for changing that user's own password until the
change is made.

### `POST /api/v1/auth/refresh`

Issue a fresh JWT from a currently valid one. Send the current token in the
`Authorization` header.

---

## Tools

### `GET /api/v1/tools`

List the tool registry. Tools are namespaced `{backend}__{tool}`.

### `PATCH /api/v1/tools/{tool_id}` — **owner**

Update a tool's enabled state or override its risk category.

```json
{ "is_enabled": false, "risk_category": "destructive" }
```

Risk overrides survive backend rediscovery.

---

## Backends

### `GET /api/v1/backends`

List backends with transport, config, health status, and tool count.

### `POST /api/v1/backends` — **owner**

Add a backend. The gateway immediately attempts to connect and discover tools.

```json
{
  "name": "filesystem",
  "transport": "stdio",
  "config": {
    "command": "npx",
    "args": ["-y", "@modelcontextprotocol/server-filesystem", "/data"],
    "env": {}
  }
}
```

See [Configuration Reference](configuration.md#backend-transports) for the
config shape of each transport.

### `PATCH /api/v1/backends/{backend_id}` — **owner**

Update a backend's configuration or enabled state.

### `DELETE /api/v1/backends/{backend_id}` — **owner**

Remove a backend, its registered tools, and any running stdio process.

### `POST /api/v1/backends/{backend_id}/sync` — **owner**

Re-run tool discovery. For agent backends this asks the connected agent to
re-register; if no agent is connected the call fails and the backend is marked
disconnected.

---

## Audit

### `GET /api/v1/audit` — **scoped**

Query audit events.

| Param | Type | Description |
|-------|------|-------------|
| `tool_name` | string | Filter by tool name |
| `backend` | string | Filter by backend name |
| `status` | string | `success`, `error`, `tool_error`, `denied`, `timeout` |
| `user_id` | uuid | Filter by user (owners only; others are pinned to themselves) |
| `risk_category` | string | `read`, `write`, `admin`, `destructive`, `execute`, `unclassified` |
| `policy_decision` | string | `allow`, `deny`, `conditional` |
| `from` / `to` | ISO 8601 | Time bounds |
| `application` | string | Filter by calling application |
| `limit` | int | Page size (default 50) |
| `offset` | int | Pagination offset |

> `tool_error` is a distinct status meaning the backend returned a result with
> `isError: true`. It is a failure, and it is counted as one in `error_count`
> and in the metrics error rate.

### `GET /api/v1/audit/export` — **scoped**

Same filters as above, clamped to at most 500 rows per call.

### `GET /api/v1/audit/stats` — **scoped**

```json
{
  "total_events": 1523,
  "events_24h": 312,
  "success_count": 1480,
  "error_count": 18,
  "denied_count": 25,
  "avg_duration_ms": 245.0,
  "top_tools": [],
  "status_breakdown": [],
  "hourly_volume": []
}
```

### `DELETE /api/v1/audit` — **owner**

Clear the audit log.

---

## Users

### `GET /api/v1/users` — **scoped**

### `POST /api/v1/users` — **owner**

```json
{ "username": "alice", "password": "…", "email": "alice@example.com", "role": "viewer" }
```

### `PATCH /api/v1/users/{user_id}`

Update `email`, `is_active`, `role`, or `password`. Owner-only, **except** that
any user may change their own password (and nothing else) on their own record.

The last *active* owner cannot be demoted or deactivated.

### `DELETE /api/v1/users/{user_id}` — **owner**

You cannot delete your own account, nor the last active owner. The check and the
delete run in one transaction, so concurrent deletes cannot strand the system
with zero administrators.

---

## Roles

| Endpoint | Notes |
|----------|-------|
| `GET /api/v1/roles` | List roles with their default policy |
| `POST /api/v1/roles` — **owner** | Create a role |
| `PATCH /api/v1/roles/{role_id}` — **owner** | Update name, default policy, or attached policies |
| `DELETE /api/v1/roles/{role_id}` — **owner** | Delete a non-system role |
| `GET /api/v1/roles/{role_id}/impact` — **owner** | Preview which users and tools a role change would affect |

---

## Policies

| Endpoint | Notes |
|----------|-------|
| `GET /api/v1/policies` | List policy rules |
| `POST /api/v1/policies` — **owner** | Create a rule |
| `PUT /api/v1/policies/{policy_id}` — **owner** | Replace a rule |
| `DELETE /api/v1/policies/{policy_id}` — **owner** | Delete a rule |

```json
{
  "name": "Block destructive ops",
  "priority": 1,
  "tool_pattern": "*__delete_*,*__remove_*",
  "decision": "deny",
  "reason": "Destructive operations are not allowed",
  "risk_categories": ["destructive"],
  "application_match": null
}
```

See [Authentication & Authorization](authentication.md#policy-engine) for how
rules are evaluated.

---

## API keys

| Endpoint | Notes |
|----------|-------|
| `GET /api/v1/api-keys` — **scoped** | List keys (metadata only — never plaintext) |
| `POST /api/v1/api-keys` | Create a key. The plaintext is returned **once**. |
| `PATCH /api/v1/api-keys/{key_id}` | Rename or enable/disable a key |
| `DELETE /api/v1/api-keys/{key_id}` | Revoke a key — takes effect immediately |
| `GET /api/v1/api-keys/by-user/{user_id}` — **owner** | Keys belonging to a user |
| `POST /api/v1/api-keys/provision/{user_id}` — **owner** | Create the per-application key set for a user |
| `POST /api/v1/api-keys/reveal/{user_id}` | Reveal stored per-application keys so a client config can be copied |
| `POST /api/v1/api-keys/rotate` | Regenerate a single per-application key |

Keys are `mcpgw_`-prefixed. Store them at creation time.

---

## Metrics

### `GET /api/v1/metrics/summary`

Dashboard metrics: call totals, 24h volume, backend and tool counts,
`avg_latency_ms`, `error_rate`, `latency_percentiles` (p50/p95/p99),
`top_tools_24h`, `backend_health`, `calls_by_risk`, and `hourly_volume`.

### `GET /metrics`

Prometheus text-format metrics, served at the **root**, not under `/api/v1`.

> This endpoint is currently unauthenticated. Do not expose it publicly — scrape
> it from an internal network or restrict it at your reverse proxy.

---

## Usage analytics

| Endpoint | Returns |
|----------|---------|
| `GET /api/v1/usage/graph` | Nodes and edges for the usage graph: users, applications, backends, tools, and the links between them |
| `GET /api/v1/usage/connections` | Connection-level usage records |

Owners may pass `user_id=all` to aggregate across every user; other callers see
only their own activity.

---

## Agent releases

Used by the agent's self-update mechanism. The gateway proxies a git forge's
release API — see `RELEASE_PROXY_URL` in the
[Configuration Reference](configuration.md).

| Endpoint | Returns |
|----------|---------|
| `GET /api/v1/agent/releases` | All agent releases (tags matching `agent-v*`) |
| `GET /api/v1/agent/releases/latest` | Latest agent release metadata |
| `GET /api/v1/agent/releases/{tag}/download?arch={target}` | Streams the binary for a target triple |

---

## MCP and WebSocket endpoints

These are not REST — they are the protocol surfaces.

### `POST /mcp`

The streamable-HTTP MCP endpoint that AI clients connect to. Accepts MCP
JSON-RPC (`initialize`, `tools/list`, `tools/call`). Authenticate with a Bearer
API key.

```jsonc
{
  "mcpServers": {
    "gateway": {
      "url": "https://mcp-gateway.example.com/mcp",
      "headers": { "Authorization": "Bearer mcpgw_your_key" }
    }
  }
}
```

### `GET /agent/ws` (WebSocket)

Where remote agents connect. See [Agent Architecture](agent-architecture.md).

### `GET /api/v1/ws/live` (WebSocket)

Live event feed powering the dashboard's real-time view. Authenticated;
non-owners receive only their own events.
