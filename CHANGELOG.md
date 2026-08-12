# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
