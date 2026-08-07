# Architecture

MCP Gateway is a self-hosted, multi-user aggregation and security layer between
AI clients and MCP tool servers. Three components plus PostgreSQL.

```
 AI clients ──MCP(HTTP)+Bearer──▶ mcp-gateway-server (Rust/Axum) ──▶ backends
 (Claude/Cursor)                    │  auth → policy → audit → route   stdio / http / sse / agent(ws)
 dashboard (React) ──/api/v1 + JWT─▶│                                  │
 agent (macOS app) ─/agent/ws + key▶│                            PostgreSQL (users, backends,
                                    │                             tools, audit, policies)
```

## Components

| Component | Path | Role |
|-----------|------|------|
| **Server** | `mcp-gateway-server/` | The only component with database access, and the auth/policy/audit authority. Everything trust-sensitive lives here. |
| **Dashboard** | `mcp-gateway-dashboard/` | A pure client of `/api/v1`. Holds no authority — the server re-validates every request. |
| **Agent** | `mcp-gateway-agent/` | A macOS app that runs on a user's machine, dials **out** to `/agent/ws`, and bridges that machine's local MCP servers into the gateway. A UI-free Rust core (`core/`) behind a C ABI (`ffi/`), linked into a SwiftUI app (`macos/`). |

The agent↔server wire protocol is defined in both crates; the guard against
drift is the golden-JSON tests in `mcp-gateway-agent/core/src/protocol.rs`,
and extracting it into a shared crate is a known follow-up.

## Request flow

A tool call from an AI client travels:

1. **Ingress** — the client POSTs an MCP JSON-RPC request to `/mcp` with a
   `Bearer` credential (a JWT or an `mcpgw_` API key).
2. **Authentication** — the credential resolves to `Claims { sub, roles }`. The
   server re-checks `is_active` and role membership against the database on
   every request; it does not trust the token's contents alone.
3. **Policy** — the policy engine evaluates the tool against the caller's role
   default and any matching policy rules, producing `allowed` or `denied`.
4. **Routing** — an allowed call is dispatched to the backend that owns the tool,
   over whichever transport that backend uses.
5. **Audit** — the call, its decision, its latency, and redacted request/response
   payloads are written to `audit_events`.

Tool names are namespaced as `{backend_name}__{original_tool_name}`, so two
backends can each expose a `search` tool without colliding.

## Backend transports

| Transport | How the server reaches it |
|-----------|---------------------------|
| `stdio` | Spawns the MCP server as a child process on the gateway host; JSON-RPC over stdin/stdout. |
| `streamable-http` | POSTs JSON-RPC to a URL. The server accepts either a JSON object or a one-shot SSE stream in reply. |
| `sse` | Opens a long-lived SSE stream for events and POSTs requests to the endpoint the stream announces. |
| `agent` | The backend lives on a remote Mac running the MCP Gateway Agent app, reached over the WebSocket that agent opened. See [Agent Architecture](agent-architecture.md). |

## Trust model

- **Untrusted:** AI clients, agent-connected machines, and downstream MCP
  backends — including their tool payloads and their responses. Backends are
  third-party code; treat their output and their environment as hostile.
- **Trusted:** the server process, PostgreSQL, and the operator.

### Roles

There are two effective privilege levels. `require_admin` gates every mutating
and global-aggregate endpoint and admits only the `owner` role
(`api/auth.rs`). Per-user data endpoints (audit, usage, api-keys, users) scope
non-owners to their own `sub`.

## Transport hardening

These are properties the server enforces, not merely recommendations:

- **stdio backends** — JSON-RPC replies are correlated by request id, so a stale
  response left in the pipe after a timeout cannot be handed to the next caller.
  Spawn logging records the argument *count*, never the arguments, which
  routinely carry tokens.
- **streamable-http backends** — the client advertises both `application/json`
  and `text/event-stream` and parses whichever the server returns, because a
  streamable-http server chooses per response.
- **SSE backends** — the `endpoint` URL announced by the (untrusted) backend is
  validated against the backend's own origin before the gateway POSTs to it
  carrying the stored `Authorization` header. Cross-origin and scheme-downgraded
  endpoints are refused, blocking both credential exfiltration and SSRF into
  internal targets.
- **Audit storage** — payloads pass through a redactor before being written,
  covering bearer tokens, labeled credentials (quoted or bare), and raw
  `mcpgw_`-prefixed gateway keys.

## Data model

The principal tables:

| Table | Holds |
|-------|-------|
| `users`, `roles`, `user_roles` | Identity and role assignment |
| `api_keys` | Hashed API keys, their owner, and last-used timestamps |
| `backends` | Backend definitions, transport config, and health status |
| `tool_registry` | Discovered tools, their namespaced names, and risk categories |
| `policies` | Policy rules — patterns, risk categories, decisions, priorities |
| `audit_events` | One row per tool call, with redacted payloads |

Migrations are applied at boot from `db/mod.rs`.

## Related

- [Authentication & Authorization](authentication.md) — credentials, roles, policies
- [Agent Architecture](agent-architecture.md) — the WebSocket protocol
- [Configuration Reference](configuration.md) — environment and backend config
