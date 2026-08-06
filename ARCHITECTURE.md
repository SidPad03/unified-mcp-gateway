# Architecture

MCP Gateway is a self-hosted, **multi-user** aggregation + security layer between AI
clients and MCP tool servers. Three components + Postgres.

```
 AI clients ──MCP(HTTP/SSE)+Bearer──▶ mcp-gateway-server (Rust/Axum) ──▶ backends
 (Claude/Cursor)                        │  auth → policy → audit → route     stdio / http / sse / agent(ws)
 dashboard (React) ──/api/v1 + JWT────▶ │                                    │
 agent (Rust TUI) ──/agent/ws + key───▶ │                              Postgres (users, backends,
                                        │                               tools, audit, policies)
```

## Component boundaries

- **mcp-gateway-server** (`mcp-gateway-server/`) — the only component with DB access and the auth/policy/audit authority. Everything trust-sensitive lives here.
- **mcp-gateway-dashboard** (`mcp-gateway-dashboard/`) — a pure client of `/api/v1`. Holds no authority; the server re-validates every request.
- **mcp-gateway-agent** (`mcp-gateway-agent/`) — runs on a user's machine, dials **out** to `/agent/ws`, and bridges that machine's local MCP servers to the gateway. The wire protocol it speaks is currently defined in both crates; extracting it into a shared crate is a known follow-up.

## Auth & trust model (multi-user)

- **Identity:** JWT (dashboard login) OR `mcpgw_`-prefixed API key. Both resolve to `Claims { sub, roles }`; the server re-checks `is_active`/roles against the DB every request.
- **Roles:** `owner` (admin) vs non-owner. `require_admin` gates mutation + global-aggregate endpoints. Per-user data endpoints (audit, usage, api-keys, users) scope non-owners to their own `sub`.
- **Trust boundaries:**
  - *Untrusted:* AI clients, agent-connected machines, downstream MCP backends (their tool payloads, their responses). Backends are third-party code — treat their output and their env as hostile.
  - *Trusted:* the server process, Postgres, the operator.

## Transport hardening notes

- **stdio backends:** JSON-RPC responses are correlated by request id, so a late or
  stale response from a previous call can't be handed to the next caller. Spawn
  logging records the argument *count*, never the arguments, which routinely carry
  tokens.
- **HTTP / streamable-http backends:** each POST is answered with either a JSON
  object or a one-shot SSE stream, decided per response, so the client advertises
  both media types and parses whichever comes back.
- **SSE backends:** the `endpoint` URL announced by the (untrusted) backend is
  validated against the backend's own origin before the gateway POSTs to it with
  the stored `Authorization` header. A cross-origin or scheme-downgraded endpoint
  is refused, which blocks both credential exfiltration and SSRF into internal
  targets.
- **Audit storage:** payloads pass through a redactor before they are written to
  `audit_events`, covering bearer tokens, labeled credentials (quoted or bare), and
  raw `mcpgw_`-prefixed gateway keys.

## Operations

- **Build & publish:** pushing to `main` triggers GitHub Actions, which builds
  multi-arch (`linux/amd64` + `linux/arm64`) server and dashboard images and pushes
  them to GHCR. Agent binaries are cross-compiled with `cargo-zigbuild` and attached
  to GitHub Releases.
- **Deploy:** pull the published images with the provided `docker-compose.yml`. Pin
  the `image:` tags to a release tag rather than `:latest` if you want reproducible
  rollouts; rolling back is then a tag change plus `docker compose up -d`.
- **A backend goes unhealthy:** check the **Backends** page health column and the
  server logs, then re-run **Sync**. Discovery failures (401/timeout) mark the
  backend `unhealthy` and leave the previously discovered tools in place until a
  sync succeeds.
- **First boot:** `JWT_SECRET` is **required** — the server refuses to start without
  it (generate one with `openssl rand -hex 32`). The seeded `admin` account requires
  a password change on first login; set `MCPGW_ADMIN_PASSWORD` to choose your own
  initial password instead. Always run behind a TLS-terminating reverse proxy.

For vulnerability reporting and the supported-version policy, see
[SECURITY.md](SECURITY.md).
