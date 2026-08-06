# Agent Architecture

> **Partly superseded.** The wire protocol, keepalive, resync and reconnect
> behaviour on this page survive unchanged into the macOS app. The *process*
> model does not: the daemon, PID file and launchd/systemd service are replaced
> by an application that owns the tunnel itself. See
> [Agent Desktop App](agent-desktop-app.md).

The gateway and each `mcp-gateway-agent` hold a persistent WebSocket between
them. That lets local MCP backends on a user's machine — stdio processes, local
HTTP servers — be exposed through the gateway without opening any inbound port
on the device.

```
┌─────────────────────┐        WebSocket         ┌──────────────────────┐
│   Gateway Server    │◀════════════════════════▶│    Gateway Agent     │
│  (cloud / homelab)  │   wss://…/agent/ws        │  (macOS/Linux/Win)   │
│                     │                           │                      │
│  ┌───────────────┐  │                           │  ┌────────────────┐  │
│  │ Agent Registry│  │                           │  │ Local Backends │  │
│  │  (in-memory)  │  │                           │  │  stdio / http  │  │
│  └───────────────┘  │                           │  └────────────────┘  │
│  ┌───────────────┐  │                           │  ┌────────────────┐  │
│  │  PostgreSQL   │  │                           │  │  config.toml   │  │
│  │ (tools, etc.) │  │                           │  │ ~/.mcp-gatew…  │  │
│  └───────────────┘  │                           │  └────────────────┘  │
└─────────────────────┘                           └──────────────────────┘
```

---

## Connection lifecycle

### 1. Connect and authenticate

The agent opens a WebSocket to the gateway, presenting its API key. The server
accepts it either as a query parameter or an `Authorization` header:

```
wss://mcp-gateway.example.com/agent/ws?token=mcpgw_xxxxxxxx
```

The key is resolved against `api_keys`. If it is invalid the server sends an
`error` message and closes the socket.

### 2. Register

Once connected, the agent sends a `register` message listing every tool it
discovered from its local backends:

```json
{
  "type": "register",
  "agent_id": "my-macbook",
  "tools": [
    {
      "name": "obsidian__read_note",
      "description": "Read a note from Obsidian",
      "inputSchema": { }
    }
  ],
  "backends": [
    { "name": "obsidian", "transport": "stdio", "command": "npx", "tool_count": 7 }
  ]
}
```

The server then:

- Creates or updates a `backends` row with `transport = 'agent'`
- Registers the tools in `tool_registry`, auto-classifying each by risk
- Replies with `registered` and the `backend_id`

The `backends` row is upserted by name, so a matching row is not required up
front — an agent whose `agent_id` is not yet known creates one on first
registration. Pre-create the backend when you want to set its risk category
before the agent ever connects; if you do, its name must match the `agent_id`
exactly, or the agent registers under a second, separate backend.

### 3. Steady state — tool calls

```
Client → Server (MCP):  tools/call { name: "obsidian__read_note", arguments: {…} }
Server → Agent  (WS) :  { "type": "tool_call", "request_id": "…", "tool": "…", "arguments": {…} }
                        Agent executes against the local stdio/http backend
Agent  → Server (WS) :  { "type": "tool_result", "request_id": "…", "result": {…} }
Server → Client (MCP):  tool result
```

Policy is evaluated and the audit event recorded **server-side, before** the
call is forwarded — an agent never sees a call the policy engine denied.

Each call runs in its own task on the agent, so concurrent calls do not block
the WebSocket read loop. The server waits up to 120 seconds for a result.

Failures come back as `tool_error`, which the gateway records with audit status
`tool_error` — a failed call, counted as an error.

### 4. Keepalive

| Mechanism | Interval | Timeout | Direction |
|-----------|----------|---------|-----------|
| Agent application ping | 20s | 45s with no inbound message ⇒ reconnect | Agent → Server |
| Server heartbeat | 30s | send failure ⇒ clean up the agent | Server → Agent |

The agent's 45-second rule keys off *any* inbound message, not just `pong`. It
exists because a TCP connection can go stale silently — device sleep/wake, WiFi
drops, route changes — and the socket looks open long after it stopped working.

### 5. Resync

Clicking **Sync** on an agent backend in the dashboard:

```
Dashboard → Server:  POST /api/v1/backends/{id}/sync
Server → Agent (WS): { "type": "resync" }
                     Agent re-discovers tools from its local backends
Agent → Server (WS): { "type": "register", … }   (full re-registration)
Server → Dashboard:  { "status": "synced", "tools_discovered": N }
```

If no agent is connected, the sync fails and the backend is marked
`disconnected`.

### 6. Disconnect

**Server side:** removes the agent from the in-memory registry, sets
`health_status = 'disconnected'`, and fails every pending tool call with
"Agent disconnected" rather than letting callers hang.

**Agent side:** enters the reconnect loop immediately.

---

## Reconnect behaviour

The agent reconnects forever, with exponential backoff capped at 30 seconds. The
connect attempt itself times out after 15 seconds so a black-holed network
cannot wedge it.

The backoff resets to 1 second only after a connection has stayed up for at
least **30 seconds**. This matters because a gateway that accepts a socket and
then immediately closes it returns a *clean* close — without the stability
requirement, that path would reset the backoff every time and produce a
reconnect storm of roughly one attempt per second.

---

## Wire protocol

### Agent → Server

| Message | Purpose |
|---------|---------|
| `register` | Announce agent id, tools, and sub-backends |
| `tool_result` | Return a successful tool call result |
| `tool_error` | Return a failed tool call |
| `ping` | Application-level keepalive |

### Server → Agent

| Message | Purpose |
|---------|---------|
| `registered` | Confirm registration, carrying `backend_id` |
| `tool_call` | Forward a tool call to execute locally |
| `pong` | Reply to the agent's ping |
| `resync` | Ask the agent to re-discover and re-register |
| `error` | Report an error to the agent |

Every message is JSON with a `type` discriminator. Calls are correlated by
`request_id`.

---

## Local backend execution

On the agent, a stdio backend is a spawned child process spoken to over
stdin/stdout in JSON-RPC.

Responses are matched to requests **by id**. That is what keeps a late reply
from a timed-out call from being handed to the next caller, and it also
discards server-initiated requests (`sampling/createMessage`, `roots/list`),
which carry ids of their own and would otherwise be mistaken for a tool result
and desynchronise the pipe for every later call.

---

## Security properties

- **Authentication** — an `mcpgw_` API key, validated on WebSocket upgrade.
- **No inbound ports** — the agent always dials out, so the device needs no
  firewall changes or port forwarding.
- **TLS** — connections use `wss://` via rustls. `tls_skip_verify` exists for
  self-signed certificates in development; leave it off anywhere else.
- **Policy still applies** — agent-hosted tools are subject to the same RBAC and
  policy evaluation as any other backend, enforced before forwarding.
- **Config at rest** — `~/.mcp-gateway-agent/config.toml` is written `0600`; it
  holds the API key and often backend credentials.
- **Verified updates** — self-update downloads are checksum-verified before
  replacing the running binary.
