# MCP Gateway Agent

A macOS app that connects the MCP servers running on your Mac to the gateway
over a single authenticated WebSocket. The gateway then treats those tools as if
they ran on the server itself.

Because the agent dials **out**, your Mac needs no inbound ports, no firewall
rules, and no port forwarding.

Requires **macOS 26 or later** on Apple Silicon or Intel. For the design and the
reasoning behind it, see [Agent Desktop App](agent-desktop-app.md).

---

## Install

Download `MCP-Gateway-Agent-<version>.dmg` from the
[latest agent release](https://github.com/SidPad03/unified-mcp-gateway/releases),
open it, and drag the app to Applications.

> **First launch needs one extra step.** The app is ad-hoc signed rather than
> notarized, so Gatekeeper will say it "cannot be opened". **Right-click the app
> and choose Open**, then confirm — macOS remembers the choice and will not ask
> again. From the command line the equivalent is:
>
> ```bash
> xattr -dr com.apple.quarantine "/Applications/MCP Gateway Agent.app"
> ```
>
> Updates installed from within the app are unaffected; quarantine only applies
> to a fresh download.

---

## Set up

Open the app, type your gateway address — `mcp-gateway.example.com` is enough;
the scheme and path are worked out for you — and click **Sign in with your
gateway**.

A browser window opens on the gateway's authorization page. Sign in with your
normal gateway account and click Authorize. The app receives its credential
directly and stores it in your Keychain.

That is the whole setup. There is no API key to create, copy, or paste, and
nothing to configure in the dashboard first — the Mac registers itself under the
machine name shown under **More options** (your computer's name, by default).

### Adding your MCP servers

**Backends → Add Backend.** For a local process choose *stdio* and give it the
command and arguments you would type in a terminal; for a local HTTP MCP server
choose *HTTP* and give it the URL.

Use **Test connection** before saving. It starts the backend, completes the MCP
handshake, lists the tools it found, and shuts it down again — so a mistyped
command is caught immediately rather than becoming a mystery later.

Backends can be added, edited, disabled and removed while the agent is
connected. The gateway is told about the change within about half a second.

---

## The app

| Page | What it shows |
|------|---------------|
| **Overview** | Connection state, tools registered, backends up, calls per hour, and whatever is currently broken |
| **Backends** | Every local MCP server: status, PID, uptime, restarts, tools, last error |
| **Activity** | Tool calls as they happen — time, tool, backend, duration, and the error if there was one |
| **Logs** | The agent's own log and every backend's stderr, merged, filterable, exportable |
| **Audit** | The gateway's audit trail for this machine, with 24-hour volume, error rate and latency |
| **Usage** | Which applications call which tools through this Mac |

**Settings** is the standard ⌘, window, in the app menu and the menu-bar popover.

### The menu bar

Closing the window does not quit the app — the Mac's MCP servers stay connected
and the app moves to the menu bar. The popover shows status and the actions you
are most likely to want; **Quit** stops the backends, and says so the first time.

### Start at login

**Settings → General → Start at login.** Registered through macOS's own login
item mechanism, so it also appears in System Settings → General → Login Items and
can be turned off there. Started that way, the app comes up in the menu bar with
no window and does not steal focus.

---

## How it works

1. The app connects to the gateway over WebSocket (`/agent/ws`) and
   authenticates with the key it received when you signed in.
2. It starts every configured local backend **concurrently** and discovers their
   tools.
3. It registers the tools of the backends that actually came up, namespaced
   `backend__tool`, under this machine's name.
4. When an AI client calls one, the gateway routes the request over the
   WebSocket.
5. The app forwards it to the right local backend and returns the result.

Each backend has a supervisor watching it. If a process exits, its tools are
withdrawn from the gateway immediately — so a client never sees a tool that
cannot answer — and it is restarted with a backoff that caps at thirty seconds.

Agent tool calls go through the gateway's policy engine and audit log exactly
like any other tool call; the decision is made server-side, before the call is
forwarded.

For the wire protocol and connection lifecycle, see
[Agent Architecture](agent-architecture.md).

---

## Where things are kept

| What | Where |
|------|-------|
| Configuration | `~/.mcp-gateway-agent/config.toml`, written atomically with `0600` permissions |
| API key | Your login Keychain, as `com.mcpgateway.agent` |
| Logs | In memory — the most recent 5 000 lines. Use **Export** on the Logs page to write them to a file |

The API key is deliberately *not* in the config file. A config written by the old
terminal agent still has one; the app moves it into the Keychain on first launch
and rewrites the file without it.

You should not need to edit the config by hand, but the schema is in
[Configuration Reference](configuration.md#agent-configuration) and there is a
worked example at `mcp-gateway-agent/mcp-gateway-agent.toml.example`.

---

## Updates

The app checks on launch and every six hours. When a release is available a small
download button appears in the window's top-right corner; the menu-bar popover
and **Settings → Updates** say so too, and Settings has the release notes and a
**Check now** button.

Clicking it downloads the new version, verifies its Ed25519 signature, replaces
the app, then quits and reopens it. That is one click — there is no separate
relaunch step, and the usual "quitting stops this Mac's MCP servers"
confirmation is skipped, because the app is coming straight back.

If a build was made without an update signing key it will tell you an update
exists and decline to install it — download the new version from GitHub instead.
That is deliberate: it never installs something it cannot verify.

---

## Troubleshooting

| Symptom | Cause |
|---------|-------|
| "The application is damaged" on first launch | Gatekeeper quarantine. Right-click → Open, or `xattr -dr com.apple.quarantine` — see [Install](#install) |
| A backend says **Failed** with "No such file or directory" | The command is not on your login shell's `PATH`. The app reads `PATH` from your login shell at launch, so a tool installed since then needs the app restarted |
| A backend keeps crashing | Open **Logs** and filter to that backend — its stderr is there, which is usually enough to see why |
| Backend stays `disconnected` in the dashboard | The machine name does not match the gateway-side backend name, or the app is not running |
| Repeated auth failures | The API key was revoked in the dashboard. Sign out and sign in again |
| TLS errors against a self-signed certificate | **Settings → Gateway → Skip TLS certificate verification.** Only on a network you trust |
| Reconnect loop | The gateway is accepting then closing the socket; check the server logs and the reverse proxy's WebSocket configuration |
| macOS asks about Keychain access after an update | Expected with ad-hoc signing — the signature changes with every build. Choose **Always Allow** |

Everything the agent logs, including each backend's stderr, is on the **Logs**
page. Secrets are redacted there using the same rules as the server's audit
redactor, so the export is safe to attach to an issue.
