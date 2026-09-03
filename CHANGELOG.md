# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.2.2] - 2026-09-03

An audit pass over the numbers, the interface and the docs. Most of what follows
is one of two shapes: a figure that disagreed with the same figure one page
over, or a boundary that held on one surface and not on its twin.

### Security

- **`GET /api/v1/metrics/summary` is owner-only.** It took an authenticated
  caller and discarded the claims, so any account — and any `mcpgw_` key —
  read the whole deployment's call volume, error rate, latency percentiles,
  user count and the names of its ten busiest tools. A non-owner whose own
  traffic was 318 calls was shown 3,581. `GET /audit/stats` is the per-user view
  of the same rows and was fixed for this exact leak in 1.0.0; this endpoint was
  not fixed with it.

- **The live feed is scoped to the caller, and re-checks the account.**
  `/api/v1/ws/live` subscribed every socket to one global channel and forwarded
  every frame, so any authenticated user watched every other user's tool names,
  backends, applications and error messages in real time — and the dashboard's
  Usage page folded those calls into counts the server had scoped to one user,
  so the figure on screen climbed all session and snapped back on refresh. The
  JWT branch was also a bare `decode`, skipping the `is_active` and role re-read
  every REST request performs; because a socket authenticates once and is then
  held, a deactivated account kept streaming for the token's remaining lifetime.
  Both WebSocket endpoints now go through one `resolve_bearer`, shared with the
  request extractor so the two cannot drift again.

- **A stdio backend no longer inherits the gateway's own secrets.** A child
  process inherits its parent's environment, and the gateway reads `JWT_SECRET`
  and `DATABASE_URL` out of that same environment — so a third-party MCP server,
  which is exactly what this product exists to put behind a gate, could mint an
  HS256 owner token or connect to Postgres directly, past auth, policy and the
  audit trail. A test reads `src/` and fails when the server learns to read a
  variable nobody added to the strip list.

- **`error_message` is redacted before it is stored.** The payload columns went
  through the redactor and this one did not, though it is the only
  payload-derived field any surface actually displays. The strings are built
  from a backend's own response body — `Backend returned HTTP 401: {…}` — which
  is where an upstream echoes back the credential the gateway just sent it. The
  same field was already redacted on its way to the live feed, so the two
  surfaces disagreed about whether it had been.

- **Both redactors catch a credential nested inside a JSON string.** A tool
  whose argument or result is itself JSON writes the separator as an escaped
  quote, and the pattern could not cross it: `{"args":"{\"api_key\": \"sk-…\"}"}`
  went through untouched. A scheme word before the value (`Basic YWRt…`) and
  Base64 `=` padding were missed too. The server's redactor and the agent's now
  share a corpus of the shapes a real gateway sees, so neither can drift.

- **A non-owner cannot read a backend's command line or URL.** The redaction
  stripped `env` and `headers` and stopped there — while this product's own
  Connect flow writes `["-y", "mcp-remote", url, "--header", "Authorization:
  Bearer …"]` into `args`, and an SSE URL can carry a session token in its query
  string. The Backends page is a non-owner's landing route and every row expands
  on click.

- **An agent registration cannot take over a backend that is not an agent.**
  `/agent/ws` accepts any active API key and then takes the `agent_id` verbatim
  from the frame that follows. The upsert was keyed on name alone and forced
  `transport = 'agent'`, so a register frame naming an existing stdio or HTTP
  backend rewrote that row, destroyed its stored environment block and its
  tokens, and repointed every call for that backend at the socket that sent the
  frame. Binding an agent id to the key issued for it is a larger change and is
  still to come.

- **`PATCH /api/v1/tools/{id}` validates what it is given.** It accepted any
  string as a risk category and would reclassify one of the gateway's own
  internal tools — the two guards `gateway_set_tool_classification` already
  applied. A category off the ladder matches no policy, so a rule scoped to
  `destructive` silently stops governing the tool.

### Fixed

- **"Tools on this backend" is one number again.** It was written out by hand in
  three places with three different filters: the Backends page excluded internal
  tools, Metrics → Backend health excluded nothing, the Usage graph excluded
  disabled tools instead. One connected Mac read 6, 15 and 15 on three pages of
  the same gateway, and `filesystem` read 12, 12 and 11. Every caller now takes
  the pair from one helper — `registered` and `enabled` — and the Usage node's
  count is derived from the tool rows the same response carries, so its label
  and its picture cannot disagree. The Backends page's headline figure now
  counts what is actually behind the gate: disabling a backend used to leave it
  where it was. This also replaced the `COUNT(*)`-per-backend loop with one
  grouped query.

- **`gateway_get_health` counts the tools the operator put there.** The one
  gateway-wide tool total that never got the `is_internal` filter 1.2.1 promised
  — 47 where the Metrics page one click away said 38, and nine higher per
  connected Mac.

- **The Security posture checklist exists.** The card called
  `GET /api/v1/security/posture`, which the server has never served. The `catch`
  written to avoid claiming a posture it could not read is what made the total
  absence invisible: on a security product, an operator with an unrotated seeded
  admin password read a Metrics page with nothing flagged, indistinguishable
  from one that checked and found nothing. The endpoint now reports the three
  signals the gateway can actually answer, and the card renders a warning rather
  than deleting itself when the read fails.

- **"Calls by risk" stops dropping a bucket and renormalising to 100%.** The
  server reported an unreviewed tool's calls as `unknown`; the chart filtered a
  whitelist that only knew `unclassified` and computed its percentages over the
  survivors, so 6.5% of the traffic vanished from the bar with the remaining
  slices still summing to a tidy 100%. Any category outside the six did the
  same. The server now uses the word the classifier, the policy editor and the
  risk ramp all use, and the chart draws whatever it is sent.

- **A migration runs once.** There was no version table, so every block executed
  on every boot. 006 reset the owner role's default policy, so an operator who
  set it to `deny` found it back at `allow` after the next restart; 012 deleted
  audit rows, so a backend named `gateway` lost its history every time the
  container came up. 010 had solved this by hand with an `information_schema`
  check and a comment explaining why; `run_migrations` now solves it for all of
  them. An upgraded database runs everything one final time and then records it.

- **`gateway` is a reserved backend name.** `api/mcp.rs` asserted it "is not a
  name a backend can take"; neither write path checked, and the REST create
  validated the name not at all.

- **One slow backend no longer keeps the gateway from starting.** Discovery ran
  serially and was awaited before the listener bound, and the SSE handshake was
  the one outbound call with no timeout — on a client that set none either. A
  backend that completed its TCP handshake and then went quiet meant no
  dashboard, no `/api/v1` and no `/metrics`; because the compose healthcheck
  curls `/metrics`, the dashboard container never started either. The listener
  binds first and discovery is spawned behind it.

- **An agent backend is `disconnected` after a restart.** Nothing wrote that
  status when the *gateway* restarted, so a Mac that was switched off came back
  `healthy`, counted as healthy on the Backends page, and had its whole tool set
  advertised over `tools/list` until someone tried to call one.

- **A reconnect no longer unregisters the connection that replaced it.** The
  cleanup removed whatever handle held the agent id, so after a network flap the
  Mac was genuinely connected while every call to it answered "Agent not
  connected". Each connection now carries an epoch and cleanup compares it
  first.

- **The audit trail pages in a stable order.** `ORDER BY timestamp DESC` with no
  tiebreaker over rows that share a millisecond let an offset page repeat a row
  or skip one.

- **Export exports what is on screen, and says when it is short.** The button
  asked `/audit` for 10,000 events against a handler that clamps to 500, wrote
  those 500 to the file, and dropped every active filter — next to a Clear
  dialog that printed the *filtered* count beside an unconditional `TRUNCATE`.
  `/audit/export` now takes the same filters and reports `{ total, truncated }`,
  and the Clear dialog says plainly that every event goes.

- **A failed request is not an empty ledger.** The Audit page rendered "No calls
  recorded yet" and "0 events recorded" under its own error banner, and the
  Usage page swallowed the failure entirely and drew "No usage data yet" — so an
  unreachable gateway reported itself as a healthy idle one, on the one page a
  non-owner can open. A first load that fails is now the page; a later failure
  is a banner over figures marked stale.

- **A stale response is discarded.** Three range buttons mean three requests in
  flight and they do not return in order; on the Usage graph the five-second
  poll made it routine rather than a race you had to force. Audit, Usage and
  Metrics each carry a request-sequence guard.

- **Searching the audit trail resets the page.** Every other filter did.
  Searching from page 4 asked for offset 60 into a ten-row result, so the table
  said "Nothing matches those filters" while the header above it counted the
  matches — and the pager that would have explained it had just disappeared.

- **The Usage graph is readable in the light theme.** Every node painted a
  literal near-black gradient while the text inside it used the theme's ink, so
  in light mode the page that answers "what is talking to what" was near-black
  on near-black.

- **Two tables on the Users page fit a phone.** Roles and API keys carried seven
  and six unranked columns; at 375px they overflowed their container by 214 and
  240 pixels, and the column that went was the one holding Edit, Delete and
  Revoke. A table inside `overflow-x-auto` hides that from a document-overflow
  check, which is why it survived.

- **The documented reverse proxy routes `POST /mcp`.** `location ~ ^/(api|mcp)/`
  needs a slash after the group and the gateway serves `/mcp` exactly, so the
  request fell through to the dashboard and an MCP client was answered with
  `index.html` and HTTP 200 — the failure `nginx.conf` already documents at
  length for `/agent/ws`. The dashboard's own Connect dialog compounded it,
  defaulting every generated client config to `https://localhost:8080/mcp`: the
  dashboard's port, over a scheme it does not serve, at a path nginx did not
  proxy. Both fixed, and `nginx.conf` gained the location.

- **The host-migration runbook names a database that exists.** It ran `pg_dump
  -U mcpgw mcpgw`, twenty lines after the backup section correctly uses
  `mcpgateway`.

- **`UPDATE_CHECK_DISABLED` reads its value, and compose passes it.** Presence
  was the test, so setting it to `false` disabled the check. It, along with
  `UPDATE_CHECK_REPO` and `GITHUB_TOKEN`, was documented as a deployment setting
  that the compose file never handed to the server, so an air-gapped operator
  who set it in `.env` kept calling GitHub.

### Changed

- **The Tools page says what its figures count.** Its headline read "Calls
  routed · 24h" and summed the tools below it, which is a different number from
  the Metrics page's figure under the identical label — 761 against 969 on the
  same window. "Disconnected", sitting beside "Backends", counted tools. A tool
  on a disabled backend read "Enabled".

- **Settings draws the risk ramp everything else draws.** It carried a private
  map painting `read` in the accent — the colour that means healthy everywhere
  in this product — and `write`, `execute` and `unclassified` in one identical
  grey, so a tool reclassified `write` → `execute` showed the same colour on
  both sides of the arrow, in the table an operator uses to review the change.

- **The volume chart's axis says which day.** Seven days buckets by hour, and
  168 points all labelled `14:00` cannot place a spike. Its Y ticks also went
  through `fmt` — left raw, the axis read `1203` beside a tooltip reading
  `1,203`.

- **A dead filter is gone.** The Audit page's Client dropdown was empty on every
  deployment, because nothing writes `audit_events.client_id`, and the parameter
  it sent had no counterpart on the server. Its status list also offered
  `timeout`, which the recorder never writes.

- Every page sets its own `<title>`, there is a skip link, and `Mono` — the
  primitive every identifier goes through — carries `translate="no"`, so browser
  auto-translate leaves tool and backend names alone.

- Documentation corrections throughout. `SECURITY.md` described a `JWT_SECRET`
  development default that does not exist and claimed only a hash of an API key
  is stored, when an encrypted copy is stored and is revealable;
  `authentication.md` said every call is audited, which 1.2.1 made false for the
  gateway's own tools; `ARCHITECTURE.md` promised an `owner` check on the
  `agent_*` namespace that has never existed there; `self-configuration.md`
  contradicted itself within fifteen lines; `/audit/stats?backend=` was
  documented as taking a UUID against SQL that matches a name; the policy
  example carried a `priority` the endpoint drops on the floor, which is how a
  deny rule created from the reference lands behind the seeded catch-all allow
  and never fires.

### Added

- **`GET /api/v1/security/posture`** — owner-only, the signals behind the
  Metrics page's checklist: whether the listener is on a public interface,
  whether a seeded account still owes a first-login password change, and who
  holds the owner role.

- **`backends[].enabled_tool_count`** alongside `tool_count`, so a caller can
  tell what a backend published from what a call can still reach.

- **`truncated`** on both audit responses, so a client writing a file can tell a
  complete export from a slice.

- **`GET /api/v1/audit/export` takes the same filters as `GET /api/v1/audit`.**

### Note for API callers

Three response contracts moved, all of them because the old one was wrong:
`metrics/summary` now requires the `owner` role, `calls_by_risk` reports an
unreviewed tool as `unclassified` rather than `unknown`, and a non-owner no
longer receives `command`, `args` or `url` in a backend's config. A backend
named `gateway` can no longer be created, and `PATCH /tools/{id}` refuses a risk
category that is not on the ladder.

## [1.2.1] - 2026-09-02

### Changed

- **The gateway's own tools are invisible on the gateway.** They shipped in
  1.2.0 as ordinary tools: listed on the **Tools** page, counted on **Backends**
  and **Metrics**, and written to the audit trail on every call. That was the
  wrong shape. A gateway is for the tools its operator put behind it, and
  seventeen `gateway_*` rows plus nine per connected Mac padded every figure on
  the dashboard and buried real traffic under configuration chatter.

  They are now marked internal and left out of the tool inventory, the
  per-backend and per-gateway counts, the audit trail, the live feed, the usage
  graph and the Prometheus endpoint alike. What does not change: they are still
  offered over MCP, still resolved, still policy-evaluated and still gated on
  the `owner` role. Only the recording is skipped — and every internal call is
  written to the *server* log at INFO with the caller, the tool, the status and
  the duration, so `docker logs` still answers "who reconfigured this, and
  when". Worth being explicit about the trade: an admin action on the gateway is
  no longer in the audit trail you can query from the dashboard, so ship the
  server log somewhere you keep it if you need those records.

  The flag is `tool_registry.is_internal`, set from a fixed list of names
  matched whole — a backend of yours that happens to ship a tool called
  `agent_something` is yours, and stays visible. Migration 012 adds the column,
  backfills it, and clears the internal rows an upgraded database already has in
  its audit trail.

- **A tool's rail carries its risk.** The badge said `write` in violet while the
  3px rail down the left of the same row stayed grey, so the column you scan
  peripherally only ever separated "admin or destructive" from "everything
  else". Both now come from one definition of the ramp, which is also what the
  charts use — a legend and the bar beside it disagreeing is worse than either
  being wrong alone.

- **The gateway's own tools cannot reclassify themselves.**
  `gateway_set_tool_classification` refuses an internal tool. Their categories
  are what a "deny destructive" policy matches on to keep them governed; letting
  one drop itself to `read` would have undone that in a single call.

- **Two error messages had fourteen stray spaces in them,** from a line
  continuation `cargo fmt` folded into the literal. The agent-backend edit
  refusal was one of them.

### Added

- **A switch for the gateway's tools** — **Settings → Gateway tools**, or
  `PATCH /api/v1/settings` for the API. Owner-only, defaults to on, and read per
  request: turning it off withdraws the namespace from the next `tools/list`,
  and a call to one of the tools is answered as an unknown tool, because that is
  what it now is. Each Mac keeps its own separate switch for the tools that
  configure it.

- **`GET` / `PATCH /api/v1/settings`,** and a `settings` table behind them, for
  the handful of switches that belong to the deployment rather than to the
  browser looking at it.

## [1.2.0] - 2026-09-02

### Added

- **The gateway can be configured through its own tools.** A `gateway_*`
  namespace sits alongside everything the gateway routes: register, update,
  remove, probe, start, stop and restart backends; write, reprioritise and
  delete RBAC policy; reclassify a tool; tail a backend's stderr; search the
  audit trail; read a health snapshot. Seventeen tools, so an assistant that
  finds a backend unhealthy can read its logs, fix its configuration and restart
  it without anyone opening the dashboard.

  They are not a side door. Each one carries a risk category from the same
  five-level ladder every other tool is classified on — writing policy is
  `admin`, deleting one is `destructive` — so a rule you already have ("deny
  destructive for the ci role") governs them with no special case. Everything
  that writes additionally requires the `owner` role, checked as the REST API
  checks it, and every call is recorded in the audit trail under the backend
  name `gateway`. A tool the caller cannot reach is not advertised in
  `tools/list`.

- **A Mac running the agent can be configured the same way.** A smaller mirror,
  `agent_*`, scoped to one machine: list, install, remove, start, stop, restart
  and reconfigure its MCP servers, and tail their logs. Installing runs the
  server and asks for its tool list before adopting it, so a mistyped command
  fails the call instead of leaving a broken row behind. Environment *values*
  still never leave the machine — only key names, and which of them are masked.

  Nothing that changes the tunnel itself is exposed: a call that repointed
  `gateway_url` would arrive over the connection it was about to sever. It is on
  by default, because "install and expose the Obsidian MCP server" is most of
  the reason to have an agent, and it is a real grant — so
  `agent.expose_control_tools`, and **Settings → General → Remote control**, are
  the last word for whoever owns the Mac. See
  [docs/self-configuration.md](docs/self-configuration.md).

- **A stdio backend's stderr is captured and readable.** The gateway used to let
  child processes write to its own stderr, where the output was interleaved with
  every other backend's and lost on restart. Each one now has a 500-line ring,
  redacted on the way in, that outlives the process — because "why did it die"
  is a question you only ask after it died. `gateway_get_mcp_server_logs` reads
  it, along with the backend's recent failures from the audit trail.

- **The Metrics page has 24h / 7d / 30d, like the Tools page and the usage
  graph.** Every figure on it — throughput, latency percentiles, error rate, top
  tools, calls by risk, the volume chart — follows the selected window, and the
  choice is remembered. A month-long window buckets its chart by day rather than
  drawing 720 hourly points into 190 pixels.

### Changed

- **Risk categories are five colours, not two and three greys.** `read`, `write`
  and `execute` were drawn in three shades of neutral ink, which read as
  "unimportant" rather than as three different things. The ladder is now one
  cool-to-warm sweep — azure, violet, orchid, then the amber and red `admin` and
  `destructive` already carried — in the dashboard, the macOS app, the charts
  and the usage graph alike. A risk category is a rung on a ladder rather than a
  state, so it sits outside the four tones and is the one documented exception
  to them.

- **The backend rows say they open.** A chevron sits before each backend's name
  on the dashboard's Backends page, pointing right when the row is shut and down
  when it is open — the same affordance, in the same position, as the macOS
  app's list. The row also carries `aria-expanded`.

- **An agent backend can no longer be "edited" from the dashboard.** There was
  never anything behind it: the Mac running the agent owns its command,
  environment and tool list, and re-sends all of it on every connection, so a
  save from this side was reverted without a word at the next reconnect. The
  Edit button is gone for agent backends, they are left out of the JSON editor
  with a line saying where they went, and the server refuses the write.

- **`GET /api/v1/metrics/summary` takes `?range=24h|7d|30d`,** and three fields
  were renamed with it: `calls_last_24h` → `calls_in_range`, `top_tools_24h` →
  `top_tools`, and `hourly_volume` → `volume` (its `hour` field is now
  `bucket`). The default window is unchanged, so the values a caller was getting
  are the values it still gets — only the names moved, because the old ones
  would have been wrong for any range but the first. The response also carries
  `range` and `volume_bucket`.

- **The agent's control tools are classified by name, not by keyword.**
  `agent_install_mcp_server` matches no keyword in the classifier and would have
  been filed `unclassified` — a tool that runs an arbitrary command on somebody's
  Mac, sitting outside every category-scoped policy. The names are a fixed,
  known set, so they are stated.

### Fixed

- **Streamable-HTTP backends that require a session are discovered correctly**
  ([#10](https://github.com/SidPad03/unified-mcp-gateway/issues/10)). The
  gateway sent `initialize`, `notifications/initialized` and `tools/list` as
  three independent POSTs and never read the `Mcp-Session-Id` the first one
  handed back. A stateful server had no way to know the follow-ups belonged to
  the session it had just opened, so it correctly answered `tools/list` with
  nothing and the backend registered zero tools despite a clean handshake. The
  session is now read from the initialize response and carried on everything
  that follows.

  `tools/call` learned the same thing, without paying for it on the common path:
  it still sends the call straight out, and only runs a handshake and retries
  when the server answers `400` or `404` — the two statuses the spec gives a
  session meaning. The SSE transport was never affected; it carries its session
  in the endpoint URL the server announces, which the gateway has always posted
  to.

## [1.1.0] - 2026-08-12

### Added

- **Environment variables and headers can be masked, one at a time.** Every
  variable in a backend's environment — and every header on an HTTP backend —
  now carries a lock in the editor, in the dashboard and in the macOS app alike.
  Unlocked, which is the default, the value is plain text wherever that backend
  is shown: the configuration panel, the JSON editor, the agent's server list.
  Locked, it is not rendered anywhere, and the only way back to it is to unlock
  it in the editor and save. Most of what goes in an environment is a path or a
  flag, and hiding all of it taught nobody which ones were actually secret.

  Masking is enforced where the value lives rather than in the views that show
  it. The server and the agent core both substitute a placeholder for a masked
  value on the way out, and swap the stored value back in when that placeholder
  returns on a save — so an edit that only changes a command leaves the secret
  untouched, the JSON editor round-trips it losslessly, and **Test connection**
  still starts the backend with the real value.

### Changed

- **The macOS app shows environment values it used to withhold.** Editing a
  backend presented its variables with empty fields, because the agent never
  handed a value back out; an unmasked value is now shown and edited as text.
  Existing configurations carry no masks, so everything in them starts visible —
  mask what should not be.
- **A masked value is masked for owners too.** `GET /backends` returned the full
  configuration to any owner; masked entries now come back as a placeholder for
  every caller. Non-owners still get no `env` or `headers` block at all.
- **Agents report which variables are masked** — `env_masked` in the
  registration frame — so the dashboard can mark them. Values themselves still
  never leave the machine the agent runs on; only key names ever crossed the
  wire, and that has not changed.

## [1.0.0] - 2026-08-06

The terminal agent is replaced by a macOS application.

> **Versioning was reset.** The 1.1.x line below was numbered ahead of where the
> project actually was. Server, dashboard and agent are all back on 1.0.0, and
> every release, tag and Docker Hub tag from the old line is withdrawn — so the
> entries below this one describe software with higher version numbers than this
> release. They are kept for the history, not as a sequence.

### Added

- **MCP Gateway Agent is now a macOS app.** A Rust core — the tunnel, the backend
  supervisor, config, the wire protocol — linked into a SwiftUI application
  through a C ABI. It has Overview, Backends, Activity, Logs, Audit and Usage
  pages, a menu-bar extra, a ⌘, Settings window, start at login, and signed
  self-updates. Requires macOS 26 or later; universal (Apple Silicon and Intel).
- **Sign in through the browser.** OAuth 2.0 authorization code with PKCE:
  `GET /api/v1/agent/authorize`, `POST /api/v1/agent/authorize/approve`,
  `POST /api/v1/agent/token`. Setting up a Mac is now "type your gateway address,
  sign in" — there is no API key to create, copy or paste. The credential the
  agent ends up holding is still an ordinary `mcpgw_` key, so the tunnel protocol
  is unchanged.
- **`GET /usage/graph` and `GET /audit/stats` take an optional `backend`
  filter**, so the app can scope both to the machine it runs on. On
  `/usage/graph` the filter is applied inside the SQL rather than to the results:
  the tool query takes the top 100 by call count across *every* backend, so on a
  busy gateway one machine's tools could otherwise be absent entirely.
- **Backends can be added, edited, disabled and deleted while connected**, with
  a debounced re-registration. **Test connection** starts a backend, completes
  the MCP handshake and lists its tools before anything is written to disk.
- **Every local backend has a supervisor.** A process that exits is noticed, its
  tools are withdrawn from the gateway, and it is restarted with a backoff capped
  at 30 seconds. PIDs, uptime and restart counts are visible in the app.

### Fixed

- **`GET /audit/stats` leaked across accounts.** It returned deployment-wide
  totals and the global top-tools list to any authenticated caller. It now
  applies the same non-owner scoping `GET /audit` always has.
- **A backend that crashed stayed "running" forever.** Child processes were
  stored and never awaited, so a dead backend kept its tools registered and
  failed every call until the whole agent was restarted.
- **One bad backend took the whole agent down.** Startup returned on the first
  failure; backends now start concurrently and in isolation.
- **Backend `stderr` went nowhere.** It was inherited from the parent, which in
  an app means `/dev/null` — so a server that printed a traceback and exited left
  no trace. It is captured and shown on the Logs page, with secrets redacted
  using the server's rules.
- **Concurrent calls to the same tool corrupted each other's record.** The TUI
  matched a completion to a call by tool name; correlation is by `request_id`.
- **The setup wizard reported a dead gateway as valid.** Its check was true
  whenever the connect *returned*, including connection-refused. It now inspects
  the result and reads the first frame — necessary because the gateway upgrades
  the socket before validating the token.
- **Timestamps were UTC shown as local time**, from a `secs % 86400` on the UNIX
  epoch.
- **Tools were registered for backends that had failed to start**, so the gateway
  advertised tools that could not answer.
- **Backends could not be found when launched from Finder.** A GUI app inherits
  launchd's `PATH`, not the user's, so `uvx`, `npx` and anything from Homebrew
  were missing. The app reads `PATH` from the login shell at launch.
- **The app checked for updates once and then never again.** The background
  check returned early unless the updater was `idle`, which stops being true the
  moment the first check finishes, and nothing was driving the six-hour cadence
  its own documentation described. It now polls on a timer that outlives the
  window — the app runs from the menu bar with no window open — and re-checks
  from `up to date` and `failed`.
- **Updating took two clicks and asked a misleading question.** Installing left
  the app at "ready to relaunch" waiting for a second click, and that relaunch
  went through the ordinary quit path, so it asked whether you were sure and
  warned that quitting stops this Mac's MCP servers. Installing now quits and
  reopens the app by itself, and skips that confirmation, which does not apply
  when the app is coming straight back.

### Changed

- **The server's HTTP client moved from OpenSSL to rustls.** `reqwest` was the
  only thing pulling `native-tls` — and with it `openssl-sys` and a C toolchain —
  into an otherwise pure-Rust build; `sqlx` and `jsonwebtoken` were already on
  rustls. It now uses `rustls-tls-native-roots`, which reads the **same system
  trust store**, so a private CA installed into the image (the runtime still runs
  `update-ca-certificates` on start) keeps working.

  One nuance if you use a hand-rolled internal CA: rustls validates certificates
  more strictly than OpenSSL — notably it requires a Subject Alternative Name and
  rejects certificates that rely on the deprecated Common Name fallback. A
  certificate OpenSSL accepted may be refused. Reissuing it with a SAN is the
  fix.
- **The API key moved from `config.toml` to the macOS Keychain.** An existing
  config is migrated on first launch and rewritten without it. Config writes are
  atomic (temp file + rename, `0600`).
- **CI runs on self-hosted runners.** Everything except the macOS agent app now
  builds on the `homelab` runner fleet. Since those are x86_64 and the images are
  multi-arch, the arm64 image is **cross-compiled inside the Dockerfile** rather
  than emulated, and the separate per-arch build + manifest-merge jobs collapse
  into one buildx invocation per component. Pull requests now build both
  architectures without pushing, so a broken cross-build is caught before merge.
- **The agent is no longer published as a container image.** Existing
  `sidpad03/mcp-gateway-agent` tags are left on Docker Hub but are not updated.
- The dashboard's **Add Agent** modal is gone. A Mac authorizes itself.
- **The update notice moved to the window chrome.** In the app it is a small
  download button in the top-right corner, so it is reachable from every page
  rather than only Overview, where it used to be a card competing with the hero
  for that view's one focal slot. In the dashboard it is an **Update available**
  line in the sidebar footer, shown to owners because `/settings` is an owner
  route. Both the footer and the Settings panel read one shared check, so
  pressing **Check for updates** settles the footer instead of leaving the two
  disagreeing.
- **A copy pass over the dashboard and the app.** Sentence case throughout, with
  macOS menu items left in title case per the HIG; one name per action, so a
  backend is deleted rather than removed-or-deleted depending on the surface;
  verb-first buttons; confirmations that name what is about to go; and
  placeholders that are examples rather than restatements of the label.

### Removed

- **`GET /api/v1/agent/releases*`** and the `RELEASE_PROXY_URL` /
  `RELEASE_PROXY_REPO` / `GITEA_*` variables. The app updates itself from GitHub
  Releases; the gateway is not involved.
- `install.sh`, `install.ps1`, the ratatui TUI, the CLI subcommands, the
  launchd/systemd/Task Scheduler service management, and
  `mcp-gateway-agent/Dockerfile`.

### Breaking

- **There is no terminal agent.** Linux and Windows are not covered by this
  release. `mcp-gateway-agent run`, `setup`, `service …` and the rest no longer
  exist; the `agent-v1.0.0`–`agent-v1.1.3` binaries are withdrawn. An existing
  `config.toml` is read and migrated, so backends carry over.
- The app is ad-hoc signed rather than notarized. **First launch needs
  right-click → Open**; see [docs/agent.md](docs/agent.md#install).

## [1.1.1] - 2026-07-07

### Fixed

- **Forced first-login password change could not be completed.** After setting a
  new password on the "Set your password" screen, the request was rejected with
  _"You must change your password before continuing"_ and you were bounced back
  to the same screen. The server-side gate compared the request path against the
  `/api/v1`-prefixed URL, but axum strips the nest prefix inside the router, so
  the one request allowed to clear the flag (a `PATCH` to your own user record)
  never matched and was denied. The gate now matches the correct path, and a
  regression test covers it.

### Changed

- **Container images are now multi-arch (`linux/amd64` + `linux/arm64`).** The
  server, dashboard, and agent images are built natively for both architectures
  in CI (no emulation) and published as a single manifest, so `docker compose
  up` runs them natively on Apple Silicon / arm64 hosts. The
  `platform: linux/amd64` workaround in `docker-compose.yml` is no longer
  required.

## [1.1.0] - 2026-07-02

### Fixed — critical

- **Dashboard login was broken in v1.0.0** ([#1], [#2]). `jsonwebtoken` 10's
  default `aws_lc_rs` backend needs a process-wide rustls `CryptoProvider` that
  wasn't installed, so JWT encode/decode failed at runtime — logins returned
  401/502. Switched to the pure-Rust `rust_crypto` backend. A regression test
  now covers the HS256 round-trip.

### Added

- **Usage graph — Users column.** The `/usage` graph now shows a leftmost
  column of users and which application each user accessed. Admins get an
  **"All users"** mode that aggregates activity across everyone; clicking a user
  (or a user → app edge) filters the audit panel to that user.
- **Forced first-login password change.** New `ForcePasswordSetup` screen,
  enforced server-side so it can't be bypassed by calling the API directly.
- **Dashboard error boundary** — a single component error no longer
  white-screens the whole app.

### Security

- The server now **refuses to boot unless `JWT_SECRET` is set** to a non-default
  value (≥16 chars). See _Upgrade notes_.
- Default admin is `admin` / `admin` **with a forced password change on first
  login** (`must_change_password`, enforced in the auth layer). Set
  `MCPGW_ADMIN_PASSWORD` to choose your own initial password instead.
- Backend secrets (`env` / auth `headers`) are redacted from the backends list
  for non-admin users.
- JWTs are re-validated against the database on every request, so revoked or
  deactivated users and role changes take effect immediately (and token refresh
  can no longer perpetuate stale roles).
- Login runs in constant time for unknown usernames (removes a user-enumeration
  timing side channel).
- Internal SQL/error details are no longer leaked to MCP or WebSocket clients.
- Request bodies are capped at 8 MiB; the `/metrics` handler no longer panics on
  an encode error.
- Agent: config file is written `0600` and its directory `0700`; self-update now
  requires a valid SHA-256 checksum before replacing the running binary.
- Dashboard is served with security headers (Content-Security-Policy,
  X-Frame-Options, X-Content-Type-Options, Referrer-Policy).

### Fixed

- Policy engine: the seeded "deny destructive operations" rule was unreachable
  because the broad allow rule had higher precedence — deny rules now evaluate
  first.
- `create_user` is atomic (user + role assignment in one transaction) and
  requires an explicit, valid role (no more accidental `owner`).
- `update_user` can no longer demote or deactivate the last owner (lockout
  guard, matching `delete_user`); role changes are atomic.
- Concurrency: policy-priority assignment and agent registration use atomic
  upsert/retry instead of racy check-then-write (no more raw 500s under load).
- Dashboard: guarded `JSON.parse` of stored session state (no white-screen on
  corrupt storage), double-submit guards on all mutating forms, and a failed
  login now shows an inline error instead of reloading the page.

### Upgrade notes

- **`JWT_SECRET` is now required.** Generate one (`openssl rand -hex 32`) and put
  it in a `.env` file before `docker compose up` — see `.env.example`.
  Deployments relying on the old built-in default secret will no longer start.
- On first login you'll be required to change the `admin` password before the
  dashboard or API is usable. Set `MCPGW_ADMIN_PASSWORD` to pre-set your own
  initial password and skip the forced change.

## [1.0.0] - 2026-03-21

- Initial public release: MCP aggregation gateway (server), management dashboard,
  and connecting agent.

[#1]: https://github.com/SidPad03/unified-mcp-gateway/issues/1
[#2]: https://github.com/SidPad03/unified-mcp-gateway/issues/2
[1.1.1]: https://github.com/SidPad03/unified-mcp-gateway/releases/tag/gateway-v1.1.1
[1.1.0]: https://github.com/SidPad03/unified-mcp-gateway/releases/tag/gateway-v1.1.0
[1.0.0]: https://github.com/SidPad03/unified-mcp-gateway/releases/tag/gateway-v1.0.0
