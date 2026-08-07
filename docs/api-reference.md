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
| `status` | string | `success`, `error`, `tool_error`, `denied` |
| `user_id` | uuid | Filter by user (owners only; others are pinned to themselves) |
| `risk_category` | string | `read`, `write`, `admin`, `destructive`, `execute`, `unclassified` |
| `policy_decision` | string | `allow`, `deny` |
| `from` / `to` | ISO 8601 | Time bounds |
| `application` | string | Filter by calling application |
| `limit` | int | Page size (default 50) |
| `offset` | int | Pagination offset |

> `tool_error` is a distinct status meaning the backend returned a result with
> `isError: true`. It is a failure, and it is counted as one in `error_count`
> and in the metrics error rate.

### `GET /api/v1/audit/export` — **owner**

Returns the 10,000 most recent events, newest first. Unlike `GET /audit` it
takes no filter parameters and is not scoped to the caller.

### `GET /api/v1/audit/stats`

| Parameter | Description |
|-----------|-------------|
| `backend` | One backend only. The macOS agent passes its own `backend_id` so it can show this machine's volume, error rate and latency without pulling rows |
| `user_id` | Owners only; `all` aggregates across every user |

Scoped exactly as `GET /audit` is: a non-owner sees only their own events.

> **Changed in 1.0.0.** This endpoint previously returned deployment-wide counts
> and the global top-tools list to any authenticated caller, leaking both volume
> and tool names across accounts.

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

### `GET /api/v1/users` — **owner**

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
| `GET /api/v1/roles` — **owner** | List roles with their default policy |
| `POST /api/v1/roles` — **owner** | Create a role |
| `PATCH /api/v1/roles/{role_id}` — **owner** | Update name, description, or default policy |
| `DELETE /api/v1/roles/{role_id}` — **owner** | Delete a non-system role |
| `GET /api/v1/roles/{role_id}/impact` — **owner** | Preview the users, the users who would be left roleless, and the policy-binding count a role deletion would affect |

Policies are attached to a role from the policy side — send `role_ids` when
creating or updating a policy, not when updating the role.

---

## Policies

| Endpoint | Notes |
|----------|-------|
| `GET /api/v1/policies` — **owner** | List policy rules |
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
| `POST /api/v1/api-keys` — **owner** | Create a key. The plaintext is returned **once**. |
| `PATCH /api/v1/api-keys/{key_id}` — **owner** | Rename a key. This endpoint accepts only `name`; there is no enable/disable. |
| `DELETE /api/v1/api-keys/{key_id}` — **owner** | Revoke a key — takes effect immediately |
| `GET /api/v1/api-keys/by-user/{user_id}` | Keys belonging to a user. Self or owner. |
| `POST /api/v1/api-keys/provision/{user_id}` | Create the per-application key set for a user. Self or owner. |
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
only their own activity. `range` accepts `24h`, `7d` (default) or `30d`.

`GET /usage/graph` also takes an optional **`backend`** filter, added in 1.0.0
for the macOS agent. It is applied inside the SQL rather than to the results,
which matters: the tool query takes the top 100 by call count *across every
backend*, so on a busy gateway one machine's tools can fall off the end and
never reach the client to be filtered.

---

## Updates

### `GET /api/v1/updates/check`

| Param | Description |
|-------|-------------|
| `current` | The version the caller is running, e.g. `1.0.0` |
| `force` | Skip the server's 30-minute cache |

```json
{
  "current_version": "1.0.0",
  "latest_version": "1.0.1",
  "update_available": true,
  "release_url": "https://github.com/SidPad03/unified-mcp-gateway/releases/tag/gateway-v1.0.1",
  "checked_at": "2026-08-06T10:00:00Z",
  "source_repo": "SidPad03/unified-mcp-gateway",
  "error": null
}
```

Always returns 200. When the upstream check fails, `error` is set and
`update_available` is `false` — the caller must treat those as distinct from
being up to date. Configured by `UPDATE_CHECK_REPO`, `UPDATE_CHECK_DISABLED`, and
`GITHUB_TOKEN`.

---

## Agent sign-in

How the macOS agent obtains its credential: OAuth 2.0 authorization code with
PKCE (RFC 7636), shaped for a native app the way RFC 8252 recommends. See
[Agent Desktop App §8a](agent-desktop-app.md#8a-sign-in) for the full flow and
its security properties.

| Endpoint | Auth | Purpose |
|----------|------|---------|
| `GET /api/v1/agent/authorize` | none | The approval page. Query: `agent_id`, `redirect_uri`, `state`, `code_challenge`, `code_challenge_method=S256` |
| `POST /api/v1/agent/authorize/approve` | JWT | Mints an `mcpgw_` key for the caller and returns `{ redirect_uri }` carrying a one-time code |
| `POST /api/v1/agent/token` | none | Exchanges `{ code, code_verifier }` for `{ api_key, agent_id, username, user_id, is_owner }` |

`/agent/token` is unauthenticated by design: the PKCE verifier *is* the proof,
and it is known only to the app instance that started the flow. Codes are
single-use and expire after five minutes.

`redirect_uri` is allow-listed to `mcp-gateway-agent://auth/callback` or an
`http://127.0.0.1:<port>` loopback address. Anything else is rejected — an open
redirector here would hand the authorization code to whoever asked for it.

Any authenticated user may authorize an agent, but only for themselves.
`POST /api-keys` stays owner-only because it can mint a key for any user.

> **Removed in 1.0.0.** `GET /api/v1/agent/releases*` — the release proxy the
> terminal agent used for self-update. The macOS app updates itself from GitHub
> Releases without the gateway's involvement.

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
