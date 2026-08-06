# MCP Gateway Agent

The agent connects MCP servers running on a remote machine — a laptop, dev box,
or home server — to the gateway over a single authenticated WebSocket. The
gateway then treats those tools as if they ran on the server itself.

Because the agent dials **out**, the machine needs no inbound ports, no firewall
rules, and no port forwarding.

---

## Install

**macOS / Linux**

```bash
curl -fsSL https://raw.githubusercontent.com/SidPad03/unified-mcp-gateway/main/install.sh | bash
```

**Windows (PowerShell)**

```powershell
irm https://raw.githubusercontent.com/SidPad03/unified-mcp-gateway/main/install.ps1 | iex
```

The installer downloads the binary for your platform into
`~/.mcp-gateway-agent/bin/` and adds it to your `PATH`. Binaries are also
attached to each `agent-v*` [GitHub Release](https://github.com/SidPad03/unified-mcp-gateway/releases).

---

## Set up

### 1. Create the gateway-side backend

In the dashboard, add a backend with transport `agent`. Its **name** is the
identity the agent will claim — for example `my-macbook`.

### 2. Create an API key

From the dashboard's **Settings** page. Keys are shown once.

### 3. Run the wizard

```bash
mcp-gateway-agent setup
```

It asks for the gateway URL, the API key, and your local MCP backends, then
writes `~/.mcp-gateway-agent/config.toml` with `0600` permissions.

The `agent_id` must exactly match the backend name from step 1.

For the full config-file schema, see
[Configuration Reference](configuration.md#agent-configuration).

---

## Run

```bash
# Start in the background, then stop it again
mcp-gateway-agent run
mcp-gateway-agent stop

# Stay in the foreground with plain log output (for debugging)
mcp-gateway-agent run --foreground

# Or install it as a background service — this also starts it
mcp-gateway-agent service install
```

The service uses launchd on macOS, systemd on Linux, and Task Scheduler on
Windows.

### The TUI

`mcp-gateway-agent dashboard` opens a live view showing connection status,
registered tools, recent tool calls, and logs. `run` does not — it daemonizes,
and `run --foreground` prints plain logs.

| Key | Action |
|-----|--------|
| `q` | Quit |
| `s` | Re-run setup |
| `u` | Check for updates |
| `↑` / `↓` | Scroll the log pane |

---

## Commands

| Command | Description |
|---------|-------------|
| `mcp-gateway-agent setup` | Interactive setup wizard |
| `mcp-gateway-agent run` | Start the agent in the background (`--foreground` to stay attached) |
| `mcp-gateway-agent stop` | Stop the running agent |
| `mcp-gateway-agent restart` | Stop and start again |
| `mcp-gateway-agent dashboard` | Open the live TUI |
| `mcp-gateway-agent update` | Check for and install a newer agent |
| `mcp-gateway-agent service install` | Install as a background service, and start it |
| `mcp-gateway-agent service uninstall` | Stop and remove the background service |
| `mcp-gateway-agent service status` | Show service status |
| `mcp-gateway-agent service logs` | Show service logs |
| `mcp-gateway-agent logs` | Tail the agent log file |
| `mcp-gateway-agent uninstall` | Remove the agent's config, binary, and service |
| `mcp-gateway-agent version` | Print the version |

Each of `run`, `restart`, and `dashboard` accepts `--config <path>` to use a
config file other than `~/.mcp-gateway-agent/config.toml`.

---

## Self-update

`mcp-gateway-agent update` asks the gateway for the latest agent release, and
the gateway proxies that from the configured git forge. The downloaded binary is
checksum-verified before it replaces the running one.

This requires `RELEASE_PROXY_URL` and `RELEASE_PROXY_REPO` to be set on the
server — see [Configuration Reference](configuration.md#server-environment-variables).
Without them the update endpoints are inactive.

---

## How it works

1. The agent connects to the gateway over WebSocket (`/agent/ws`) and
   authenticates with its API key.
2. It discovers tools from each configured local backend.
3. It registers those tools with the gateway under the agent's name.
4. When an AI client calls one, the gateway routes the request over the
   WebSocket to the agent.
5. The agent forwards it to the right local backend and returns the result.

Agent tool calls go through the gateway's policy engine and audit log exactly
like any other tool call — the decision is made server-side, before the call is
forwarded.

For the wire protocol and connection lifecycle, see
[Agent Architecture](agent-architecture.md).

---

## Troubleshooting

| Symptom | Cause |
|---------|-------|
| Backend stays `disconnected` | `agent_id` does not match the backend name, or the agent is not running |
| Repeated auth failures | The API key was revoked or mistyped |
| TLS errors against a self-signed cert | Set `tls_skip_verify = true` — development only |
| Tools missing after adding a backend | Restart the agent, or click **Sync** on the backend in the dashboard |
| Reconnect loop | The gateway is accepting then closing the socket; check the server logs and the reverse proxy's WebSocket configuration |

Agent logs: `mcp-gateway-agent logs`.
