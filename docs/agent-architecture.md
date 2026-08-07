# Agent Architecture

The gateway and each MCP Gateway Agent hold a persistent WebSocket between them.
That lets local MCP backends on a user's machine — stdio processes, local HTTP
servers — be exposed through the gateway without opening any inbound port on the
device.

The agent is a macOS application (see [MCP Gateway Agent](agent.md)). It owns the
tunnel itself: there is no daemon, no PID file, no launchd service and therefore
no version skew between a background process and the window in front of you.

```
┌─────────────────────┐        WebSocket         ┌──────────────────────┐
│   Gateway Server    │◀════════════════════════▶│  MCP Gateway Agent   │
│  (cloud / homelab)  │   wss://…/agent/ws        │   (macOS 26 app)     │
│                     │                           │                      │
│  ┌───────────────┐  │                           │  ┌────────────────┐  │
│  │ Agent Registry│  │                           │  │ Local Backends │  │
│  │  (in-memory)  │  │                           │  │  stdio / http  │  │
│  └───────────────┘  │                           │  │ + a supervisor │  │
│  ┌───────────────┐  │                           │  └────────────────┘  │
│  │  PostgreSQL   │  │                           │  ┌────────────────┐  │
│  │ (tools, etc.) │  │                           │  │ config.toml +  │  │
│  └───────────────┘  │                           │  │   Keychain     │  │
└─────────────────────┘                           │  └────────────────┘  │
                                                  └──────────────────────┘
```

## Inside the app

```
   SwiftUI views  ──►  AgentModel  ──►  AgentBridge
                          ▲                 │  mcpga_command(json) → json
              tick, 10 Hz │                 ▼
                    ┌─────┴─────────────────────────────┐
                    │  mcp-gateway-agent-ffi (C ABI)    │
                    ├───────────────────────────────────┤
                    │  mcp-gateway-agent-core           │
                    │  tunnel · supervisor · backends   │
                    │  config · ring buffers            │
                    └───────────────────────────────────┘
```

Everything the agent *does* is Rust, statically linked into the app. The C ABI
is four functions: start, run a JSON command, free a string, shut down — plus one
callback that delivers batched state on a ~100 ms tick. Commands block, so the
Swift side calls them off the main thread; events arrive on a single background
thread and are decoded there before the main actor sees them.

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

### Supervision

Every backend has a task watching it for as long as the app runs. Backends start
**concurrently** — the tunnel connects in parallel with them, so a slow
`tools/list` delays nothing else — and each one moves independently through
`starting → ready`, or to `failed` if it never came up.

When a process exits, the supervisor notices, withdraws that backend's tools from
the registration, records the exit status, and restarts with a backoff of 1 s
doubling to a cap of 30 s. A process that stayed up for a minute resets the
backoff, so an occasional crash does not leave the next one waiting half a
minute. Restart counts and PIDs are visible on the Backends page.

Withdrawing the tools is the part that matters to callers: the gateway is
re-registered without them, so a client is never offered a tool whose process is
gone. Registration changes are debounced by ~500 ms, so ten backends coming up at
launch produce one `register` frame rather than ten.

Each backend's `stderr` is piped into a bounded ring buffer (5 000 lines) that
feeds the Logs page, with secrets redacted on the way in using the same rules as
the server's audit redactor.

### PATH

Backends are spawned with a `PATH` read from the user's login shell once at
launch, falling back to a built-in list (`/opt/homebrew/bin`, `/usr/local/bin`,
`~/.local/bin`, `~/.cargo/bin`, …).

This is not incidental. An app launched from Finder inherits launchd's `PATH` —
`/usr/bin:/bin:/usr/sbin:/sbin` — not the user's, and every MCP server people
actually run (`uvx`, `npx`, `bun`, anything from Homebrew) lives somewhere else.
Without it, backends that work perfectly in a terminal would fail to spawn.

---

## Security properties

- **Authentication** — an `mcpgw_` API key. Note that the server upgrades the
  WebSocket *before* it validates the token and rejects a bad one with an `error`
  frame on the open socket, so a successful handshake proves reachability and
  nothing about the credential. Anything checking a gateway has to read the first
  frame.
- **Obtaining the credential** — OAuth 2.0 authorization code with PKCE, in the
  system browser. Nobody copies a key into a config file. See
  [Agent Desktop App §8a](agent-desktop-app.md#8a-sign-in).
- **No inbound ports** — the agent always dials out, so the device needs no
  firewall changes or port forwarding.
- **TLS** — connections use `wss://` via rustls. `tls_skip_verify` exists for
  self-signed certificates in development; leave it off anywhere else.
- **Policy still applies** — agent-hosted tools are subject to the same RBAC and
  policy evaluation as any other backend, enforced before forwarding.
- **Credential at rest** — the API key is in the macOS Keychain, not on disk.
  `~/.mcp-gateway-agent/config.toml` is written atomically with `0600`; it holds
  backend configuration, which often includes backend credentials of its own.
- **Arguments are never logged** — the agent logs a tool call's *argument count*,
  never the arguments. The gateway stores hashes, not payloads.
- **Verified updates** — the update archive carries an Ed25519 signature checked
  against a public key embedded in the app. A build without one reports updates
  and refuses to install them.
