# Configuration Reference

Three things get configured: the **server** (environment variables), the
**backends** it aggregates, and the **agent** (a TOML file on each machine).

---

## Server environment variables

| Variable | Default | Description |
|----------|---------|-------------|
| `JWT_SECRET` | **required** | JWT signing secret, 16 characters minimum (32+ recommended). The server refuses to boot if unset or left at the old dev default. Generate with `openssl rand -hex 32`. |
| `DATABASE_URL` | `postgresql://mcpgateway:mcpgateway@localhost:5432/mcpgateway` | PostgreSQL connection string |
| `MCPGW_ADMIN_PASSWORD` | `admin` | Initial `admin` password. If unset, defaults to `admin` and a password change is forced on first login. Setting it selects your own initial password with no forced change. |
| `LISTEN_ADDR` | `0.0.0.0:3200` | Server listen address |
| `RUST_LOG` | `mcp_gateway_server=info,tower_http=debug` | Log level filter |
| `RELEASE_PROXY_URL` | — | Git forge base URL for the agent release proxy |
| `RELEASE_PROXY_REPO` | — | Repository for agent releases, e.g. `owner/unified-mcp-gateway` |
| `GITEA_TOKEN` | — | API token for the release proxy |
| `UPDATE_CHECK_REPO` | `SidPad03/unified-mcp-gateway` | Repository the dashboard's update check queries |
| `UPDATE_CHECK_DISABLED` | unset | Set to any value to disable the update check (air-gapped deployments) |
| `GITHUB_TOKEN` | — | Optional; raises GitHub's rate limit for the update check |

`RELEASE_PROXY_URL` and `RELEASE_PROXY_REPO` also fall back to the legacy
`GITEA_URL` and `GITEA_AGENT_REPO` names. The token has no `RELEASE_PROXY_*`
spelling — it is read only as `GITEA_TOKEN`. The agent release endpoints fail
unless a URL is set, so leave them unset only if you are not using agent
self-update.

Compose reads these from a `.env` file next to `docker-compose.yml`. See
[.env.example](../.env.example).

---

## Backend transports

Backends are the MCP servers the gateway aggregates. Four transports, each
suited to a different deployment shape.

### `stdio` — local process on the gateway host

The gateway spawns the MCP server as a child process and speaks JSON-RPC over
its stdin/stdout.

Best for: MCP servers installed on the gateway host itself.

```json
{
  "name": "filesystem",
  "transport": "stdio",
  "config": {
    "command": "npx",
    "args": ["-y", "@modelcontextprotocol/server-filesystem", "/data"],
    "env": { "SOME_TOKEN": "…" }
  }
}
```

`env` entries become environment variables for the spawned process.

### `streamable-http` — remote HTTP server

The gateway POSTs MCP JSON-RPC to a URL. It advertises both `application/json`
and `text/event-stream` and parses whichever the server returns, because a
streamable-http server chooses per response.

```json
{
  "name": "my-api-server",
  "transport": "streamable-http",
  "config": {
    "url": "https://tools.example.com/mcp",
    "headers": { "Authorization": "Bearer …" }
  }
}
```

### `sse` — Server-Sent Events

For MCP servers predating the streamable-HTTP spec. The gateway opens a
long-lived SSE stream and POSTs requests to the endpoint that stream announces.

```json
{
  "name": "legacy-server",
  "transport": "sse",
  "config": {
    "url": "https://tools.example.com/sse",
    "headers": { "Authorization": "Bearer …" }
  }
}
```

> The announced endpoint is validated against the backend's own origin before
> the gateway POSTs to it with your stored `Authorization` header. A backend that
> returns a cross-origin or scheme-downgraded endpoint is refused.

### `agent` — remote machine over WebSocket

The backend is handled by a remote `mcp-gateway-agent`. The gateway holds a row
for it; when the agent connects and announces itself, its tools register
automatically.

Best for: tools on laptops, dev boxes, or any machine that cannot accept
inbound connections.

```json
{ "name": "macbook-pro", "transport": "agent", "config": {} }
```

The backend `name` must exactly match the `agent_id` in that agent's config.

### HTTP/SSE headers vs. env

HTTP and SSE backends have no subprocess, so key-value pairs configured for them
are sent as **request headers**, not environment variables. The dashboard labels
them "HTTP Headers" and stores them under `headers`; legacy records stored under
`env` are still read, and the server merges both (an explicit `headers` entry
wins). Values are masked in the UI behind a reveal toggle.

---

## Tool discovery

When a backend is added — or the server starts — the gateway queries it for a
tool list. Discovered tools are:

1. Namespaced as `{backend_name}__{original_tool_name}`
2. Auto-classified by risk category (see [Authentication](authentication.md#risk-classification))
3. Inserted into `tool_registry`, enabled by default
4. Preserved across rediscovery for any manual risk override or enabled state

Backends that fail discovery are marked `unhealthy`; previously discovered tools
remain until a sync succeeds. A failing backend never crashes the server.

### Health status

| Status | Meaning |
|--------|---------|
| `idle` | The default: newly created and not yet checked, or the backend is disabled |
| `healthy` | Connected, tools discovered. An agent backend that has registered is also `healthy` |
| `unhealthy` | Last connection or discovery attempt failed |
| `disconnected` | Agent backend whose agent has gone away |

---

## Agent configuration

The agent reads `~/.mcp-gateway-agent/config.toml`. Run
`mcp-gateway-agent setup` for an interactive wizard, or write it directly.

```toml
[agent]
agent_id = "my-macbook"                                   # must match the backend name
gateway_url = "wss://mcp-gateway.example.com/agent/ws"
api_key = "mcpgw_YOUR_API_KEY_HERE"
dashboard_url = "https://mcp-gateway.example.com"         # optional; used for self-update
tls_skip_verify = false                                   # only for self-signed certs in dev

# A stdio backend — the agent spawns this and talks JSON-RPC over stdin/stdout
[[backends]]
name = "playwright"
transport = "stdio"
command = "npx"
args = ["@playwright/mcp@latest"]

# A stdio backend that needs environment variables
[[backends]]
name = "obsidian"
transport = "stdio"
command = "npx"
args = ["obsidian-mcp-server"]

[backends.env]
OBSIDIAN_API_KEY = "your_api_key"
OBSIDIAN_BASE_URL = "http://127.0.0.1:27123/"

# An HTTP backend the agent connects to rather than spawns
[[backends]]
name = "local-tool"
transport = "http"
url = "http://localhost:9000/mcp"

[backends.headers]
Authorization = "Bearer …"
```

### Fields

**`[agent]`**

| Field | Required | Description |
|-------|----------|-------------|
| `agent_id` | yes | Identifies this agent; must match the gateway backend's name |
| `gateway_url` | yes | WebSocket URL, normally `wss://<host>/agent/ws` |
| `api_key` | yes | An `mcpgw_` API key |
| `dashboard_url` | no | Base URL for self-update; derived from `gateway_url` if omitted |
| `tls_skip_verify` | no | Skip TLS verification — self-signed certs only |

**`[[backends]]`**

| Field | Applies to | Description |
|-------|-----------|-------------|
| `name` | all | Backend name; namespaces its tools |
| `transport` | all | `stdio` or `http` |
| `command`, `args`, `env` | `stdio` | Process to spawn and its environment |
| `url`, `headers` | `http` | Endpoint and request headers |

The config file is written with `0600` permissions — it holds an API key and
often backend credentials too.
