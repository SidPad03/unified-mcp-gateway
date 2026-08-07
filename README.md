<p align="center">
  <img src="mcp-gateway.svg" alt="MCP Gateway" width="120" />
</p>

<h1 align="center">MCP Gateway</h1>

<p align="center"><strong>One connector. Total visibility. Secure tool access.</strong></p>

<p align="center">
A self-hosted MCP-native aggregation, routing, and security layer for desktop AI clients and agentic workflows. Connect all your MCP servers behind a single, centrally managed endpoint with full audit trails, RBAC, and policy enforcement.
</p>

## Demo

https://github.com/user-attachments/assets/d34467d1-8485-45d3-847f-7f9274142f7f

## Architecture

MCP Gateway is a three-component system:

```
┌─────────────────────┐     ┌─────────────────────────────────────────┐
│  AI Client          │     │  MCP Gateway Server (Rust/Axum)         │
│  (Claude, Cursor,   │────▶│                                         │
│   etc.)             │ MCP │  ┌─────────┐ ┌────────┐ ┌───────────┐  │
└─────────────────────┘     │  │ Router   │ │ Policy │ │ Audit     │  │
                            │  │ & Tools  │ │ Engine │ │ Recorder  │  │
                            │  └────┬─────┘ └────────┘ └───────────┘  │
                            │       │                                  │
                            │  ┌────┴──────────────────────────────┐  │
                            │  │         Backend Manager           │  │
                            │  │  stdio | http | sse | agent(ws)   │  │
                            │  └──┬─────────┬──────────┬───────────┘  │
                            └─────┼─────────┼──────────┼──────────────┘
                                  │         │          │
                            ┌─────┴──┐ ┌────┴───┐ ┌───┴──────────┐
                            │ Local  │ │ Remote │ │ MCP Gateway  │
                            │ stdio  │ │ HTTP   │ │ Agent (WS)   │
                            │ MCP    │ │ MCP    │ │              │
                            │ Server │ │ Server │ │ ┌──────────┐ │
                            └────────┘ └────────┘ │ │local MCP │ │
                                                  │ │servers   │ │
                                                  │ └──────────┘ │
                                                  └──────────────┘
```

### Components

| Component | Tech | Description |
|-----------|------|-------------|
| **mcp-gateway-server** | Rust, Axum, PostgreSQL | Core gateway — MCP protocol routing, auth, policy enforcement, audit, metrics |
| **mcp-gateway-dashboard** | React, TypeScript, Vite | Admin UI — tool inventory, audit timeline, metrics charts, user/policy management |
| **mcp-gateway-agent** | Rust core + SwiftUI | macOS app — connects your Mac's MCP servers to the gateway over WebSocket |
| **PostgreSQL** | PostgreSQL 16 | Persistent storage for users, backends, tools, audit events, policies |

## Quick Start

### 1. Start the gateway

The server and dashboard are published as prebuilt images on Docker Hub, so
there's no build step — just pull and run. Grab the compose file, set a JWT
secret, and start:

```bash
# Download the compose file
curl -O https://raw.githubusercontent.com/SidPad03/unified-mcp-gateway/main/docker-compose.yml

# A JWT secret is required — the server refuses to boot without one
echo "JWT_SECRET=$(openssl rand -hex 32)" > .env

# Pull the prebuilt images and start everything (server + dashboard + postgres)
docker compose up -d
```

This starts three containers:
- **MCP Gateway Server** on port 3200
- **Dashboard** on port 8080
- **PostgreSQL** (internal to the compose network)

The images are:

| Component | Image |
|-----------|-------|
| Server | [`sidpad03/mcp-gateway-server`](https://hub.docker.com/r/sidpad03/mcp-gateway-server) |
| Dashboard | [`sidpad03/mcp-gateway-dashboard`](https://hub.docker.com/r/sidpad03/mcp-gateway-dashboard) |

> The agent is no longer published as a container image. It is a macOS app,
> released as a `.dmg` — see [Remote Agent](#remote-agent). Historical
> `sidpad03/mcp-gateway-agent` tags are left on Docker Hub but are not updated.

> Each image is a multi-arch manifest (`linux/amd64` + `linux/arm64`), so it
> runs natively on Intel and Apple Silicon / arm64 hosts with no emulation. The
> compose file tracks `:latest` — to pin a release, change the `image:` tags to
> a version like `:v1.0.0`. CI keeps the five most recent version tags plus
> `latest`, so pin to a version you have actually deployed. Prefer to build from
> source? See [Development](#development).

### 2. Log in to the dashboard

Open http://localhost:8080 and log in with the default credentials: `admin` / `admin`.

You'll be **required to set a new password on first login** before you can use
the dashboard — the default is only for initial setup. (To skip the default and
set your own initial password, put `MCPGW_ADMIN_PASSWORD=...` in your `.env`.)

### 3. Add an MCP backend

In the dashboard's **Backends** page, add a backend. For example, to add the GitHub MCP server:

| Field | Value |
|-------|-------|
| Name | `github` |
| Transport | `stdio` |
| Command | `npx` |
| Args | `-y @modelcontextprotocol/server-github` |
| Env | `GITHUB_TOKEN=ghp_your_token` |

The gateway will start the backend and register its tools automatically.

### 4. Connect your AI client

Point your MCP client (Claude Desktop, Cursor, etc.) at the gateway's MCP endpoint:

```jsonc
// Claude Desktop config (~/.claude/claude_desktop_config.json)
{
  "mcpServers": {
    "gateway": {
      "url": "http://localhost:3200/mcp",
      "headers": {
        "Authorization": "Bearer <your_api_key>"
      }
    }
  }
}
```

The dashboard's **Backends → Connect a client** builds this block for you, with a
key for the client you pick. All backends' tools are then available through this
single endpoint.

### Production deployment

For production, put strong secrets in a `.env` file (git-ignored — Compose reads
it automatically):

```env
# Required — the server will not start without it
JWT_SECRET=your-strong-random-secret
# Optional — defaults to `mcpgateway` if unset
POSTGRES_PASSWORD=your-strong-db-password
# Optional — if unset, defaults to `admin` with a forced change on first login
MCPGW_ADMIN_PASSWORD=your-initial-admin-password
```

Then:

```bash
docker compose up -d
```

Always deploy behind a TLS-terminating reverse proxy (nginx, Caddy, etc.) in production.

## Remote Agent

**MCP Gateway Agent** is a macOS app that connects the MCP servers running on
your Mac to the gateway over a single authenticated WebSocket. The gateway sees
them as if they were running on the server itself. Because the app dials **out**,
your Mac needs no inbound ports and no firewall changes.

Requires macOS 26 or later, Apple Silicon or Intel.

### Install

Download `MCP-Gateway-Agent-<version>.dmg` from the
[latest agent release](https://github.com/SidPad03/unified-mcp-gateway/releases)
and drag it to Applications.

> **First launch needs one extra step.** The app is ad-hoc signed rather than
> notarized, so macOS will say it "cannot be opened". **Right-click the app and
> choose Open**, then confirm — macOS remembers the choice. From a terminal:
> `xattr -dr com.apple.quarantine "/Applications/MCP Gateway Agent.app"`.
> Updates installed from inside the app are unaffected.

### Set up

Open the app, type your gateway address, and click **Sign in with your gateway**.
A browser window opens on the gateway's authorization page; sign in with your
normal account and click Authorize.

That is the whole setup. There is no API key to create, copy, or paste, and
nothing to configure in the dashboard first — the Mac registers itself.

Add your MCP servers from **Backends → Add backend**. **Test connection** starts
the backend, completes the MCP handshake and lists the tools it found before
anything is saved, so a mistyped command is caught immediately. Backends can be
added, edited, disabled and removed while the agent is connected.

### What it shows

Connection state and tool counts; every local backend with its status, PID,
uptime, restarts and last error; tool calls as they happen; the agent's log and
every backend's stderr, merged and exportable; and the gateway's audit trail and
usage graph, scoped to this machine.

Closing the window keeps the app running in the menu bar. **Settings** is the
standard ⌘, window.

### How it works

1. The app connects to the gateway via WebSocket (`/agent/ws`)
2. It starts every local backend concurrently and discovers their tools
3. It registers the tools of the backends that came up, under this machine's name
4. When an AI client calls one, the gateway routes the request over the WebSocket
5. The app forwards it to the right local backend and returns the result

Each backend has a supervisor watching it: if a process exits, its tools are
withdrawn from the gateway immediately — so a client is never offered a tool that
cannot answer — and it is restarted with a capped backoff.

All tool calls go through the gateway's policy engine, RBAC, and audit logging —
even for remote agent tools.

Full documentation: [docs/agent.md](docs/agent.md). Design and rationale:
[docs/agent-desktop-app.md](docs/agent-desktop-app.md).

## Features

### MCP Aggregation & Routing
- Connect multiple MCP backends behind a single endpoint
- Supports **stdio**, **streamable-http**, **SSE**, and **agent** (WebSocket) transports
- Automatic tool namespacing: `{backend}__{tool}` with collision resolution
- Centralized tool registry with enable/disable per tool

### Security & Access Control
- **JWT + API Key** authentication (API keys use `mcpgw_` prefix, SHA-256 hashed)
- **RBAC** — a built-in `owner` role plus any roles you create, each with a default allow/deny and its own attached policy rules
- **Policy Engine** — Priority-ordered allow/deny rules with glob patterns, risk categories, and per-application matching
- **Risk Classification** — Tools auto-classified as `read`, `write`, `admin`, `destructive`, `execute`, or `unclassified`
- **Audit Logging** — Every tool call recorded with configurable redaction

### Observability
- **Prometheus metrics** at `/metrics` — call counts, latency histograms, error rates, backend health
- **Metrics dashboard** with charts for volume, latency, and per-tool breakdowns
- **Usage graphs** with time-series analysis

### Remote Agent System
- **MCP Gateway Agent**, a macOS app, runs on your Mac
- Connects local MCP servers (stdio/http) to the gateway via authenticated WebSocket
- Browser sign-in (OAuth 2.0 + PKCE) — no API key to copy; the credential lives in the Keychain
- Backends added, edited and removed live, with a supervisor that restarts crashed ones
- Live view of connection state, tool calls, merged logs, audit and usage
- Auto-reconnect with exponential backoff
- Start at login, and signed self-updates from GitHub Releases

### Dashboard Pages

| Page | Description |
|------|-------------|
| Backends | MCP server management with health indicators, and the ready-to-paste client config |
| Tools | All aggregated tools with search, risk badges, enable/disable |
| Policies | Priority-ordered allow/deny rules, reordered by drag |
| Usage | Which users and applications call which tools, as a graph |
| Audit | Chronological event feed with drill-down details |
| Metrics | Charts for call volume, latency, error rates, backend health |
| Users | Users and roles, with per-user API keys |
| Settings | Gateway URL, AI risk classification, version and update check |

## API Reference

All endpoints under `/api/v1`. Auth via `Authorization: Bearer <jwt_or_api_key>`.

| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/auth/login` | Authenticate, returns JWT |
| POST | `/auth/refresh` | Refresh JWT token |
| GET | `/tools` | List all tools |
| PATCH | `/tools/{id}` | Enable/disable tool |
| GET | `/backends` | List backends with health |
| POST | `/backends` | Add backend |
| PATCH/DELETE | `/backends/{id}` | Update/delete backend |
| GET | `/audit` | Query audit events |
| GET | `/audit/stats` | Aggregated audit statistics |
| GET | `/metrics/summary` | Metrics dashboard data |
| GET | `/usage/*` | Usage analytics |
| GET/POST | `/users` | User management |
| GET/POST | `/roles` | Role management |
| GET/POST/PUT/DELETE | `/policies` | Policy CRUD |
| GET/POST/DELETE | `/api-keys` | API key management |
| GET | `/agent/authorize` | Agent sign-in approval page (OAuth 2.0 + PKCE) |
| POST | `/agent/authorize/approve` | Approve an agent sign-in, minting its key |
| POST | `/agent/token` | Exchange an authorization code for the agent's API key |

### MCP Endpoints

| Endpoint | Description |
|----------|-------------|
| POST `/mcp` | Streamable HTTP MCP endpoint |
| WS `/agent/ws` | Agent WebSocket connection |
| WS `/api/v1/ws/live` | Live event feed for the dashboard |

## Environment Variables

### Server

| Variable | Default | Description |
|----------|---------|-------------|
| `DATABASE_URL` | `postgresql://mcpgateway:mcpgateway@localhost:5432/mcpgateway` | PostgreSQL connection string |
| `JWT_SECRET` | **required** | JWT signing secret (≥16 chars). The server refuses to boot if unset or left at the old dev default. Generate with `openssl rand -hex 32`. |
| `MCPGW_ADMIN_PASSWORD` | `admin` | Initial `admin` password. If unset, defaults to `admin` and a password change is forced on first login. Set it to choose your own initial password (no forced change). |
| `LISTEN_ADDR` | `0.0.0.0:3200` | Server listen address |
| `RUST_LOG` | `mcp_gateway_server=info,tower_http=debug` | Log level filter |

## Development

```bash
# Backend (requires Rust + PostgreSQL)
cd mcp-gateway-server
cargo run

# Dashboard (requires Node.js)
cd mcp-gateway-dashboard
npm install
npm run dev

# Agent core and its C ABI (no macOS needed)
cd mcp-gateway-agent
cargo test --workspace

# The macOS app (needs macOS 26 and the Command Line Tools — no Xcode required)
./macos/build.sh            # builds build/MCP Gateway Agent.app
open "build/MCP Gateway Agent.app"
```

## Deployment

The project includes CI/CD via GitHub Actions:

- **Server & Dashboard**: multi-arch Docker images (`linux/amd64` + `linux/arm64`), built natively per-architecture and published to **Docker Hub** (`sidpad03/mcp-gateway-*`) on every `main` push. Old tags are pruned automatically — the five newest versions plus `latest` are retained
- **Agent app**: built universal on a `macos-26` runner and published as a `.dmg` on an `agent-v*` GitHub Release, alongside a signed `appcast.json` the app self-updates from

Publishing requires two repository secrets — `DOCKERHUB_USERNAME` and
`DOCKERHUB_TOKEN`, an access token with **Read/Write/Delete** scope. Delete is
needed by the tag-retention step.

See [CONTRIBUTING.md](CONTRIBUTING.md) for local development setup and deployment details.

## Documentation

Full documentation lives in [docs/](docs/):

| Page | What it covers |
|------|----------------|
| [Architecture](docs/architecture.md) | System design, request flow, transports, trust model |
| [Deployment Guide](docs/deployment.md) | Production deployment, TLS, backups, upgrades |
| [Configuration Reference](docs/configuration.md) | Environment variables, backend transports, agent config |
| [Authentication & Authorization](docs/authentication.md) | JWTs, API keys, roles, policy engine |
| [API Reference](docs/api-reference.md) | Every REST, MCP, and WebSocket endpoint |
| [MCP Gateway Agent](docs/agent.md) | Installing, configuring, and running the agent |
| [Agent Architecture](docs/agent-architecture.md) | The agent↔server WebSocket protocol |

## Changelog

See [CHANGELOG.md](CHANGELOG.md) for the list of changes, fixes, and upgrade notes in each release.

## Security

Please see [SECURITY.md](SECURITY.md) for information on reporting vulnerabilities and security considerations.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines on contributing to this project.

## License

Apache 2.0
