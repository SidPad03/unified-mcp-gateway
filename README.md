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
| **mcp-gateway-agent** | Rust, ratatui TUI | Remote agent — connects local MCP servers to the gateway over WebSocket |
| **PostgreSQL** | PostgreSQL 16 | Persistent storage for users, backends, tools, audit events, policies |

## Quick Start

### 1. Start the gateway

The server and dashboard are published as prebuilt images on GHCR, so there's no
build step — just pull and run. Grab the compose file, set a JWT secret, and start:

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

> The compose file tracks `:latest`. To pin a specific release, change the
> `image:` tags to `:v1.1.0`. Prefer to build from source? See
> [Development](#development).

### 2. Log in to the dashboard

Open http://localhost:8080 and log in with the default credentials: `admin` / `admin`.

You'll be **required to set a new password on first login** before you can use
the dashboard — the default is only for initial setup. (To skip the default and
set your own initial password, put `MCPGW_ADMIN_PASSWORD=...` in your `.env`.)

### 3. Add an MCP backend

In the dashboard's **Backend Config** page, add a backend. For example, to add the GitHub MCP server:

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

Generate an API key from the dashboard's **Settings** page. All backends' tools are now available through this single endpoint.

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

The **MCP Gateway Agent** lets you connect MCP servers running on remote machines (laptops, dev boxes, home servers) to the gateway over a single authenticated WebSocket. The gateway sees the agent's local MCP servers as if they were running on the server itself.

### Install the agent

**macOS / Linux:**

```bash
curl -fsSL https://raw.githubusercontent.com/SidPad03/unified-mcp-gateway/main/install.sh | bash
```

**Windows (PowerShell):**

```powershell
irm https://raw.githubusercontent.com/SidPad03/unified-mcp-gateway/main/install.ps1 | iex
```

The installer downloads the correct binary for your platform, puts it in `~/.mcp-gateway-agent/bin/`, and adds it to your `PATH`.

### Configure the agent

Run the interactive setup wizard:

```bash
mcp-gateway-agent setup
```

This walks you through entering your gateway URL, API key, and adding local MCP backends. The config is saved to `~/.mcp-gateway-agent/config.toml`.

You can also edit the config file directly. Here's an example with three backends:

```toml
[agent]
agent_id = "my-macbook"
gateway_url = "wss://mcp-gateway.example.com/agent/ws"
api_key = "mcpgw_YOUR_API_KEY_HERE"
dashboard_url = "https://mcp-gateway.example.com"
tls_skip_verify = false   # only set true for self-signed certs in dev

# A stdio backend — the agent spawns this process and talks JSON-RPC over stdin/stdout
[[backends]]
name = "playwright"
transport = "stdio"
command = "npx"
args = ["@playwright/mcp@latest"]

# Another stdio backend with environment variables
[[backends]]
name = "github"
transport = "stdio"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-github"]

[backends.env]
GITHUB_TOKEN = "ghp_your_token_here"

# An HTTP backend — the agent connects to an already-running MCP server
[[backends]]
name = "obsidian"
transport = "stdio"
command = "npx"
args = ["obsidian-mcp-server"]

[backends.env]
OBSIDIAN_API_KEY = "your_obsidian_api_key"
OBSIDIAN_BASE_URL = "http://localhost:27123/"
```

### Run the agent

```bash
# Start with the live TUI dashboard
mcp-gateway-agent run

# Or run in the background as a system service
mcp-gateway-agent service install
mcp-gateway-agent service start
```

The TUI dashboard shows connection status, registered tools, recent tool calls, and logs in real time. Press `q` to quit, `s` to re-run setup, `u` to check for updates.

### Agent commands

| Command | Description |
|---------|-------------|
| `mcp-gateway-agent setup` | Interactive setup wizard |
| `mcp-gateway-agent run` | Connect to gateway with live TUI |
| `mcp-gateway-agent dashboard` | Open the TUI dashboard only |
| `mcp-gateway-agent update` | Check for and install updates |
| `mcp-gateway-agent service install` | Install as a background service (launchd/systemd/Task Scheduler) |
| `mcp-gateway-agent service start` | Start the background service |
| `mcp-gateway-agent service stop` | Stop the background service |
| `mcp-gateway-agent service status` | Check service status |
| `mcp-gateway-agent logs` | Tail the agent log file |
| `mcp-gateway-agent version` | Print version |

### How it works

1. The agent connects to the gateway via WebSocket (`/agent/ws`)
2. It discovers tools from all its local backends (stdio and HTTP)
3. It registers those tools with the gateway under the agent's name
4. When an AI client calls a tool, the gateway routes the request over WebSocket to the agent
5. The agent forwards the call to the correct local backend and returns the result

All tool calls go through the gateway's policy engine, RBAC, and audit logging — even for remote agent tools.

## Features

### MCP Aggregation & Routing
- Connect multiple MCP backends behind a single endpoint
- Supports **stdio**, **streamable-http**, **SSE**, and **agent** (WebSocket) transports
- Automatic tool namespacing: `{backend}__{tool}` with collision resolution
- Centralized tool registry with enable/disable per tool

### Security & Access Control
- **JWT + API Key** authentication (API keys use `mcpgw_` prefix, SHA-256 hashed)
- **RBAC** — Owner, operator, and viewer roles with tool-level permissions
- **Policy Engine** — Priority-ordered allow/deny rules with glob patterns, risk categories, and per-application matching
- **Risk Classification** — Tools auto-classified as `read`, `write`, `admin`, or `external-api`
- **Audit Logging** — Every tool call recorded with configurable redaction

### Observability
- **Prometheus metrics** at `/metrics` — call counts, latency histograms, error rates, backend health
- **Metrics dashboard** with charts for volume, latency, and per-tool breakdowns
- **Usage graphs** with time-series analysis

### Remote Agent System
- **mcp-gateway-agent** binary runs on remote machines
- Connects local MCP servers (stdio/http) to the gateway via authenticated WebSocket
- TUI dashboard with live connection status, tool call tracking, and logs
- Auto-reconnect with exponential backoff
- Self-update mechanism via the gateway's release proxy
- macOS launchd service management for background operation

### Dashboard Pages

| Page | Description |
|------|-------------|
| Tool Inventory | All aggregated tools with search, risk badges, enable/disable |
| Audit Timeline | Chronological event feed with drill-down details |
| Metrics Overview | Charts for call volume, latency, error rates, backend health |
| Usage Graph | Time-series usage analysis |
| Backend Config | MCP server management with health indicators |
| Policy Editor | Security rule management with condition builder |
| User Management | User CRUD with role assignment |
| Settings | API keys, system configuration |

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
| PUT/DELETE | `/backends/{id}` | Update/delete backend |
| GET | `/audit` | Query audit events |
| GET | `/audit/stats` | Aggregated audit statistics |
| GET | `/metrics/summary` | Metrics dashboard data |
| GET | `/usage/*` | Usage analytics |
| GET/POST | `/users` | User management |
| GET/POST | `/roles` | Role management |
| GET/POST/PUT/DELETE | `/policies` | Policy CRUD |
| GET/POST/DELETE | `/api-keys` | API key management |
| GET | `/agent/releases/*` | Agent release proxy |

### MCP Endpoints

| Endpoint | Description |
|----------|-------------|
| POST `/mcp` | Streamable HTTP MCP endpoint |
| GET `/sse` | SSE MCP transport |
| WS `/agent/ws` | Agent WebSocket connection |

## Environment Variables

### Server

| Variable | Default | Description |
|----------|---------|-------------|
| `DATABASE_URL` | `postgresql://mcpgateway:mcpgateway@localhost:5432/mcpgateway` | PostgreSQL connection string |
| `JWT_SECRET` | **required** | JWT signing secret (≥16 chars). The server refuses to boot if unset or left at the old dev default. Generate with `openssl rand -hex 32`. |
| `MCPGW_ADMIN_PASSWORD` | `admin` | Initial `admin` password. If unset, defaults to `admin` and a password change is forced on first login. Set it to choose your own initial password (no forced change). |
| `LISTEN_ADDR` | `0.0.0.0:3200` | Server listen address |
| `RUST_LOG` | `mcp_gateway_server=info,tower_http=debug` | Log level filter |
| `RELEASE_PROXY_URL` | — | Git forge URL for agent release proxy (e.g., Gitea, GitHub) |
| `RELEASE_PROXY_REPO` | — | Repository for agent releases (e.g., `owner/unified-mcp-gateway`) |
| `RELEASE_PROXY_TOKEN` | — | API token for release proxy authentication (also reads `GITEA_TOKEN`) |

## Development

```bash
# Backend (requires Rust + PostgreSQL)
cd mcp-gateway-server
cargo run

# Dashboard (requires Node.js)
cd mcp-gateway-dashboard
npm install
npm run dev

# Agent
cd mcp-gateway-agent
cargo run -- setup    # interactive setup wizard
cargo run -- run      # connect to gateway
```

## Deployment

The project includes CI/CD via GitHub Actions:

- **Server + Dashboard**: Docker images built and pushed to GHCR on every `main` push
- **Agent**: Cross-compiled for macOS, Linux, and Windows via `cargo-zigbuild`, published as GitHub Releases

See [CONTRIBUTING.md](CONTRIBUTING.md) for local development setup and deployment details.

## Changelog

See [CHANGELOG.md](CHANGELOG.md) for the list of changes, fixes, and upgrade notes in each release.

## Security

Please see [SECURITY.md](SECURITY.md) for information on reporting vulnerabilities and security considerations.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines on contributing to this project.

## License

Apache 2.0
