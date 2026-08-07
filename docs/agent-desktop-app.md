# MCP Gateway Agent → macOS Desktop App

> **Status: built.** The terminal agent is gone; `mcp-gateway-agent/` is now a
> Rust core, a C ABI over it, and a SwiftUI application. This document is the
> plan of record and the design rationale — see [MCP Gateway Agent](agent.md)
> for how to use it and [Agent Architecture](agent-architecture.md) for the wire
> protocol.

---

## 1. Goal

One `.app` on a Mac that:

- keeps the machine's local MCP backends connected to the gateway (the tunnel),
- does everything the TUI + CLI did, without a terminal,
- adds Logs, Audit, Usage (scoped to this machine) and Backend add/remove,
- shares the gateway's visual identity while feeling native to macOS,
- updates itself from GitHub Releases.

Non-goals for v1: Linux, Windows, App Store distribution, a headless Linux agent.

---

## 2. Stack: SwiftUI over a Rust core

| Option | Verdict |
|---|---|
| **SwiftUI + Rust core over a C ABI** | **Chosen.** Real Liquid Glass (`glassEffect`, `GlassEffectContainer`, concentric radii, materials the OS composites), native `MenuBarExtra`, `SMAppService`, Swift Charts, lazy `List`s that virtualize for free, and no webview process at all. `core/` — the tunnel, the supervisor, config, protocol — stays in Rust and is linked in as a static library, so none of the hard logic was rewritten. |
| Tauri v2 | Rejected, after initially being chosen. Its case rested on visual parity with the dashboard being nearly free; against that, Liquid Glass in a webview is an imitation (`backdrop-filter: blur(24px)` plus a fake specular hairline) and the §7a performance rule exists *only* to work around how expensive that imitation is. Still the right answer if Windows or Linux ever become goals — see §16. |
| Electron | Rejected. ~150 MB, higher idle RAM, five to eight processes, and the Rust tunnel would have to be rewritten in Node or shipped as a sidecar. |
| Local web UI + daemon | Rejected. That is a web app, not an application. |

**On the original SwiftUI rejection.** The first draft of this document rejected
SwiftUI as "a full rewrite of the tunnel + backend manager in Swift". That
premise was wrong. The core compiles to a `staticlib` and is reachable through
four `extern "C"` functions; the FFI shim is ~450 lines and its surface is
roughly what the Tauri IPC commands would have been anyway. What SwiftUI
genuinely costs is the design system by hand (§7a), the usage graph without
`@xyflow/react` (§7), and an updater without `tauri-plugin-updater` (§9).

---

## 3. Process model

**The app owns the tunnel.** No separate daemon, no IPC socket, no version skew.

- Dock icon while a window is open; closing the window keeps the app alive in the
  menu bar. `AppDelegate` switches `NSApp.activationPolicy` between `.regular`
  and `.accessory` as windows come and go.
- Menu-bar extra: status dot, agent id, tools/backends/calls, Open, Reconnect,
  Open Dashboard, Check for Updates, Settings, Quit.
- Quit confirms the first time — it stops the machine's backends — with a
  "Don't ask again" suppression button.
- **Start at login** via `SMAppService.mainApp`. The item appears in System
  Settings → General → Login Items under the app's own name, and the app reads
  back `.requiresApproval` so it can say when the user has turned it off there.
  Launched as a login item, it comes up in the menu bar with no window and no
  focus steal (detected from the `kAEOpenApplication` event's
  `keyAELaunchedAsLogInItem` property).
- Rejected: a headless launchd daemon + UI client. Two binaries, two versions, an
  IPC protocol to design and secure, and a whole class of "daemon not running"
  failures — for a resilience gain that "start at login" already covers. This is
  how Tailscale / Docker Desktop / Ollama ship on macOS.

---

## 4. Repo layout

```
mcp-gateway-agent/
├── Cargo.toml                 # workspace: core + ffi
├── core/                      # mcp-gateway-agent-core — pure Rust, no UI
│   └── src/{config,protocol,tunnel,supervisor,logbuf,redact,state,backends/*}.rs
├── ffi/                       # mcp-gateway-agent-ffi — staticlib, C ABI
│   └── src/lib.rs
├── macos/                     # the SwiftUI app
│   ├── Package.swift  build.sh  Resources/Info.plist
│   └── Sources/MCPGatewayAgent/{App,Bridge/*,Design/*,Views/*}.swift
└── mcp-gateway-agent.toml.example
```

The `core` split is not cosmetic. Keeping every piece of logic in a crate with no
UI or platform dependency means `ci.yml` runs fmt / clippy / test against the
whole agent workspace on the existing ubuntu runner, and it leaves the door open
to a headless binary or a second platform UI without another rewrite.

**Design tokens are transcribed, not shared.** `Design/Theme.swift` carries the
dashboard's token values (the phosphor accent `#3FD69B`, the cold-steel surface
ladder, the three lamps, the nine-step type scale, the four radii) as SwiftUI
`Color`s, each with a light-appearance counterpart. A comment in both files
notes they must stay in sync.

**One logo, everywhere.** `Design/BrandMark.swift` is a direct transcription of
`brand/mcp-gateway-mark.svg` as a SwiftUI `Shape`, so the sidebar, the welcome
screen, the About pane and the menu-bar fallback all draw the same Aperture
Gate at the same proportions. No SF Symbol stands in for the logo anywhere.

---

## 5. Deleted

| Path | Why |
|---|---|
| `mcp-gateway-agent/src/tui/**` (1 597 lines) | Replaced by the app |
| `mcp-gateway-agent/src/cli.rs`, `main.rs` | No subcommands |
| `mcp-gateway-agent/src/service/{macos,linux,windows}.rs` | Replaced by `SMAppService` |
| `mcp-gateway-agent/src/update/mod.rs` | Replaced by the in-app updater |
| `install.sh`, `install.ps1` | Replaced by the DMG |
| `mcp-gateway-agent/Dockerfile` + its `ci.yml` matrix rows | D2 — no longer published |
| `mcp-gateway-server/src/api/agent_releases.rs` (+ `agent_release_cache`) | D3 — nothing consumes it once the TUI is gone |
| The dashboard's **Add Agent** modal | D5 — a Mac authorizes itself; nobody mints a key by hand |
| GitHub releases `agent-v1.0.0` … `agent-v1.1.3` and their tags | Requested. **Publish the new app first** — see §13 |

---

## 6. Core refactor — and the defects it fixes

Every item below was verified in the old source.

| # | Defect | Fix |
|---|---|---|
| 1 | `start_all` was sequential and awaited each backend's `initialize` + `tools/list` (30 s and 60 s timeouts). Ten backends could block startup for minutes. | All backends start concurrently, one supervisor task each; the UI paints immediately and each row moves `starting → ready / failed`. |
| 2 | `start_all` returned `Err` on the **first** failing backend, so `run_foreground` exited — one bad command took the whole agent down. | Per-backend isolation. A failure is a row with an error, never a process exit. |
| 3 | stdio children were stored as `_child` and never awaited. A backend that died was invisible forever. | A supervisor task per backend watches for exit, surfaces `crashed`, restarts with capped backoff, and exposes PID / uptime / restart count. |
| 4 | `stderr` was `Stdio::inherit()` — in an app, `/dev/null`. | Piped into a per-backend ring buffer feeding the Logs page. |
| 5 | `LocalBackendManager` was immutable after `start_all`; backends could not be added or removed without a restart. | `RwLock` state + `add / remove / update / restart / set_enabled`, with a debounced (~500 ms) re-`register`. |
| 6 | The TUI matched a completion to a call **by tool name**, so two concurrent calls to the same tool updated the wrong record. | Correlate by `request_id`. |
| 7 | `chrono_now()` computed `secs % 86400` on the UNIX epoch — UTC time-of-day shown as local time. | Timestamps cross the boundary as RFC 3339 UTC; the app formats them where the timezone is actually known. |
| 8 | The setup wizard's gateway check was `timeout(..).await.is_ok()` — true whenever the connect *returned*, including connection-refused. | `check_gateway` inspects the inner `Result` **and** reads the first frame, because the gateway upgrades the socket before it validates the token and rejects a bad one with an `error` message on the open connection. |
| 9 | The API key sat in plaintext in `config.toml`. | macOS Keychain, migrated on first launch. |
| 10 | Sub-backends were registered as if all were healthy, even one that failed to start. | Only backends that actually came up are registered, with their real tool counts. |
| 11 | *(new)* An app launched from Finder inherits launchd's `PATH`, not the user's — so `uvx`, `npx` and anything from Homebrew would not be found. The old agent never hit this because it only ran from a shell. | `init_login_path()` asks the user's login shell for its `PATH` once at launch, with a 3 s timeout and a built-in fallback list. |

Also added to the core:

- **Bounded ring buffers** (5 000 log lines, 1 000 tool calls) — the source of
  truth, so the app holds no unbounded history.
- **Event coalescing**: one delta message on a ~100 ms tick, and the full
  snapshot only when a generation counter says something changed. A chatty
  backend cannot flood the UI.
- **Redaction on the way in**, using the server's rules verbatim, because Copy
  and Export are where a credential would leave the machine.
- **Atomic config writes** (temp file + `rename`, `0600`).
- Unchanged: the wire protocol, reconnect/backoff behaviour, JSON-RPC id
  correlation, and the "log argument counts, never arguments" rule.

---

## 7. UI

Shell: `NavigationSplitView` with a 232 pt sidebar (the same 232 as the
dashboard), the phosphor accent `#3FD69B`, 10 pt uppercase section headers
tracked at 1.6, SF Symbols for navigation and the Aperture mark for identity.

Navigation stays a native `List`, because a Mac sidebar's selection is an
OS-drawn glass capsule and a web-style marker would fight the platform. The gate
rail — the product's signature, a 3 pt edge carrying a row's verdict — is applied
to the *data* instead: backend rows, activity rows, log lines, audit rows, and
the Overview's "needs attention" panel.

| Page | Contents | Data source |
|---|---|---|
| **Overview** | Connection hero (state, gateway, agent id, uptime), tools registered, backends up/down, calls-per-hour chart, Reconnect / Re-register / Open Dashboard, and either what is broken or what is running | Local (instant) |
| **Backends** | Status dot, transport, tool count, PID, uptime, restarts, last error; Add (stdio/http with env + headers), Edit, Remove, Restart, Enable, **Test connection** before save | Local |
| **Activity** | Live tool calls: time, tool, backend, duration, ok/err, expandable error | Local |
| **Logs** | Agent + per-backend stderr merged; level/source filters, search, follow-tail, Copy, Export, Reveal in Finder; secrets redacted with the server's rules | Local |
| **Audit** | Server audit events for this machine, 24 h volume chart, filters and a detail sheet | `GET /audit?backend=<agent_id>` |
| **Usage** | `Application → Agent → Backend → Tool` graph plus busiest-tools and application charts | `GET /usage/graph?backend=<agent_id>` |
| **Welcome** | One field and one button: gateway address → browser sign-in | §8a |
| **Settings** (⌘,) | Account, gateway URL, machine name, TLS-skip, start at login, updates, About, config path | Local |

Settings is a `Settings` scene, not a page in the window: on a Mac, "where do I
change the server address" has one answer, and it is ⌘,.

Performance rules, enforced in review:

- All long lists are lazy `List`s — a viewport of rows whether the buffer holds
  fifty or five thousand.
- **Nothing derived is computed inside `body`.** `body` re-runs on every
  observed change and the event tick is 10 Hz; filtering five thousand log lines
  there is fifty thousand predicate evaluations a second to redraw a screenful.
  Views memoize against `logRevision` / `callRevision`, and the Logs page
  maintains its filtered view incrementally — a tick costs O(lines added).
- Server-backed pages poll **only while visible and focused** (`.task` is
  cancelled on disappear; `controlActiveState` gates the loop), incrementally
  (`from=<newest seen>`), and stop when hidden.
- JSON from the core is decoded on the emitter thread, off the main actor.
- Rust release profile: `opt-level = "s"` with fat LTO — a few hundred KB of DMG
  for lower call latency.

---

## 7a. Liquid Glass

macOS 26's design language: surfaces are translucent *materials* that blur and
refract what sits behind them, edged with a specular highlight; nested shapes use
concentric corner radii; chrome floats on its own plane above the content;
everything adapts to appearance and wallpaper. Glass is for **chrome** — content
stays opaque and legible.

**Implementation, layer by layer**

| Layer | Technique |
|---|---|
| Window | `.windowStyle(.hiddenTitleBar)`, so the traffic lights float over our own sidebar header (the Finder/Mail look) |
| Sidebar | `.listStyle(.sidebar)` — the native sidebar material, composited by the OS |
| Cards, sheets, popovers | `.glassEffect(.regular, in: .rect(cornerRadius: 16))` |
| Identity tiles | `.glassEffect(.regular.tint(accent.opacity(0.22)), in: .rect(cornerRadius: …))` |
| Buttons | `.buttonStyle(.glass)` / `.glassProminent` |
| Grouping | `GlassEffectContainer`, so adjacent glass surfaces blend rather than stacking |
| Corner radii | Concentric: inner radius = outer radius − padding. Cards 16, nested rows 10, window 12 |

**The performance rule changed character, and it is worth saying why.** In a
webview, `backdrop-filter` is expensive, compounds per element, and had to be
banned outright inside virtualized lists. Natively the OS composites materials on
the GPU and the per-element cost is not the problem. What remains is a *design*
rule rather than a performance one: glass on a scrolling row is visual noise, and
rows that each re-composite as they move look unsettled. So the rule stands —
glass on chrome, never on log rows, audit rows or tool-call rows — but skipping
it now costs taste rather than frames.

**Reconciling with the dashboard.** Identity is unchanged — same accent, same
type scale, same status colours, same card rhythm, same section-header
treatment. What changes is only *how a surface is painted*. Side by side they
read as one product, one native to the Mac and one to the browser.

**Light and dark.** The dashboard is dark-only. A Mac app that ignores the system
appearance looks broken, so every token in `Theme.swift` is an `NSColor` with a
dynamic provider and both values; the native materials follow the system on their
own. Dark stays the primary design target.

**Accessibility.** Reduce transparency and reduce motion are handled by the
system for native materials — which is most of the reason to use them rather
than paint an imitation. Status is never colour-only: every dot has a label
beside it.

---

## 7b. Logo

The old mark was a lucide `Zap` bolt in `#a855f7` — generic, and it said nothing
about what the product does.

**"Aperture mark":** three traces converge on a single point and pass through a
diamond threshold, leaving as one beam. It states the product in one glyph: many
MCP servers, one guarded endpoint. Drawn on lucide's 24 grid at stroke 2 with
round caps, so it sits correctly beside the lucide icons in the dashboard and the
SF Symbols in the app, and it still reads at 16 px.

**Asset set** in `brand/`: `mcp-gateway-mark.svg`, `mcp-gateway-wordmark.svg`,
`agent-app-icon.svg`, `agent-tray-Template.svg`.

Two details that are easy to get wrong:

- The tray icon **must** be a macOS template image — pure black plus alpha, named
  with the `Template` suffix — or it will not invert on a light menu bar and will
  not highlight when clicked. `build.sh` exports it at 22 and 44 px with the
  suffix intact.
- The Dock icon is a **filled, heavier** cut of the mark on a tinted glass tile.
  A 2 px line glyph scaled to 1024 looks thin and weak next to Apple's icons.

`build.sh` rasterises both from the SVGs with `qlmanage` and `sips` and builds
the `.icns` with `iconutil` — all Command Line Tools, no extra dependency.

---

## 8. Server changes

All additive and backward compatible.

1. **`GET /usage/graph` gained an optional `backend` filter.** Required, not
   optional: the query has `LIMIT 100` on tools ordered by call count **across
   all backends**, so on a busy gateway this machine's tools can be absent
   entirely. Client-side filtering cannot fix that. The filter is applied before
   the limit, in every sub-query.
2. **`GET /audit/stats` gained the same optional `backend` filter**, so the app
   can show its own 24 h volume, error rate and latency without pulling rows.
3. **`GET /audit/stats` now applies the same non-owner user scoping `/audit`
   already applied.** It previously returned global totals and the global
   top-tools list to any authenticated caller — a pre-existing leak of both
   volume and tool names across accounts.

**Scoping semantics (deliberate):** the app authenticates with the agent's own
`mcpgw_` API key, so it sees exactly what that account would see in the
dashboard, narrowed to this machine. If the key belongs to an owner, that is
every user's calls to this agent; if not, only that user's. Both the Audit and
Usage pages carry the scope line — see D4.

---

## 8a. Sign-in

`mcp-gateway-server/src/api/agent_auth.rs`. OAuth 2.0 authorization code with
PKCE (RFC 7636), shaped for a native app the way RFC 8252 recommends.

```
app                          browser                     gateway
 │  open /agent/authorize ──────►                            │
 │      ?code_challenge=S256(v)  │ ── GET ──────────────────► │  approval page
 │                               │ ◄─ sign in, "Authorize" ─► │
 │                               │ ── POST …/approve ───────► │  mints an mcpgw_ key
 │  ◄── redirect …?code=…&state=─┘                            │
 │  POST /agent/token {code, code_verifier} ────────────────► │  verifies the challenge
 │  ◄── { api_key, username, is_owner } ──────────────────────┘
```

- The app uses `ASWebAuthenticationSession`, so sign-in happens in a real Safari
  context — an existing dashboard session is reused, a password manager works —
  and the custom-scheme redirect is intercepted without a loopback HTTP server.
- The credential the agent ends up holding is still an ordinary `mcpgw_` API
  key, so the tunnel and its authentication are unchanged. What is gone is the
  human step of creating one and pasting it into a config file.
- The approval page is served by the API, not the dashboard, so sign-in works on
  a deployment where the dashboard is absent or on another origin. It derives the
  API prefix from its own path rather than hard-coding `/api/v1`, so it survives
  a reverse proxy.
- `redirect_uri` is allow-listed to the app's scheme or a loopback address; an
  open redirector here would hand the code away. Codes are single-use with a
  five-minute lifetime, the challenge is compared in constant time, and pending
  authorizations are capped so an unauthenticated caller cannot grow the map.
- **Any authenticated user may authorize an agent, but only for themselves.**
  `POST /api-keys` is owner-only because it can mint a key for *any* user; this
  is strictly narrower.

---

## 9. Updates — signed, straight from GitHub

- The public key is Ed25519, embedded in `Info.plist` at build time; the private
  key is a repository secret. Verification is `CryptoKit`, so there is no
  third-party updater dependency and `swift build` needs no network.
- Stronger than the old SHA-256 check: a hash published beside a file proves the
  download was not corrupted and nothing else — whoever can replace the archive
  can replace the hash. The signature covers authorship.
- **The endpoint must not be `releases/latest/download/…`.** This repo
  interleaves `gateway-v*` and `agent-v*` tags, so "latest release" is regularly
  a gateway release with no app in it, which would break the updater. CI instead
  force-updates a permanent, non-"latest" release tagged **`agent-latest`** whose
  only asset is `appcast.json`:

  ```
  https://github.com/SidPad03/unified-mcp-gateway/releases/download/agent-latest/appcast.json
  ```

- Auto-check on launch; a quiet banner on Overview and in the menu bar, with
  release notes in Settings → Updates. Install downloads the `.app.tar.gz`,
  verifies the signature, unpacks it, and hands the swap to a detached script
  that waits for the app to exit, `ditto`s the new bundle over the old one, and
  reopens it.
- **A build without a public key reports updates and refuses to install them.**
  That is the right way round: it never silently installs something unverified.
- **Gatekeeper.** Without Developer ID signing + notarization, a downloaded DMG
  is quarantined and macOS says the app is damaged. In-place updates are
  unaffected (no quarantine attribute is applied), but first install is not. CI
  signs and notarizes when `APPLE_*` secrets exist and falls back to ad-hoc
  signing with a documented right-click → Open otherwise. See D1.

---

## 10. CI/CD

**`.github/workflows/agent-app.yml`** — `push` and `pull_request` on
`mcp-gateway-agent/**`, plus `workflow_dispatch`:

1. `macos-26` runner. Liquid Glass needs the macOS 26 SDK; `macos-14` cannot
   build this app. Rust targets `aarch64-apple-darwin` + `x86_64-apple-darwin`,
   `Swatinem/rust-cache`.
2. Version computed by counting `agent-v*` releases (today's scheme). With every
   release and tag deleted and all three components reset, the first release of
   the app is **`agent-v1.0.0`** — the earlier `1.1.x` line was numbered ahead of
   where the project actually was.
3. `macos/build.sh --universal --dmg` does the whole build: cargo for both
   targets, `lipo`, `swift build --arch arm64 --arch x86_64`, icon generation,
   bundle assembly, `Info.plist` patching, `codesign`, `hdiutil`.
4. Optional: import the Apple certificate into a temporary keychain, notarize,
   staple. Both gated on the secrets existing — and gated via **job-level** env,
   because a step-level `env:` is not in scope for that step's own `if:`, which
   is a quiet way to write a gate that never fires.
5. Publish `agent-v<version>` with the DMG and the update archive, then
   force-update `agent-latest`'s `appcast.json`.

**`ci.yml` changes:**

- fmt / clippy / test now run against the whole `mcp-gateway-agent` workspace on
  the ubuntu runner. Both crates are free of UI and platform dependencies, so
  this works exactly as it does on a Mac.
- The agent stays out of the Docker `build` / `merge` / `prune` matrices (D2).

**CodeQL — kept.** Worth recording where it lives, because it caused some
confusion: it is GitHub's *default setup*
(`dynamic/github-code-scanning/codeql`), a repository setting rather than a file
in `.github/workflows/`, so it cannot be found by reading the pipeline. Its runs
were failing with `The job was not acquired by Runner of type hosted even after
multiple attempts` across every language matrix. That turned out to be the
GitHub Actions outage of 2026-08-06, not a finding against this code, so the
scan stays enabled.

One detail for whoever next touches that configuration: Rust cannot be named
explicitly in the `languages` array of
`PATCH /repos/{owner}/{repo}/code-scanning/default-setup` — the API rejects it.
Rust support is in preview and is picked up by auto-detection instead, so the
configuration must be written *without* a `languages` list or Rust will be
dropped from the scan. Swift is likewise auto-detected.

No E2E in CI: driving a SwiftUI app needs XCUITest and a real UI session.
Replaced by a manual QA checklist (§12) run against the built DMG.

---

## 11. Docs

`docs/agent.md` rewritten for the app; `docs/agent-architecture.md` keeps the
wire protocol and gains the new process model and the FFI boundary; `README.md`
agent section and component table ("Rust, ratatui TUI" → "Rust core + SwiftUI,
macOS app"); `ARCHITECTURE.md` diagram; `CONTRIBUTING.md` dev loop
(`macos/build.sh`); `CHANGELOG.md` entry noting the breaking change and the
removed endpoints; `.env.example` / `docs/configuration.md` /
`docs/api-reference.md` lose `RELEASE_PROXY_*` and `GITEA_*` (D3) and gain the
sign-in endpoints.

---

## 12. Testing

**Rust (72 tests, in CI):** config round-trip, 0600 permissions, atomic write
leaves no temp files, legacy-config parsing and Keychain migration;
golden-JSON tests pinning every wire message against the server's enum shapes
(the protocol is duplicated across crates — this is the guard); tool namespacing
and routing; JSON-RPC id correlation including a server-initiated request
mid-stream; supervisor crash detection, restart, and tool withdrawal; one
failing backend not stopping the others; stderr reaching the log buffer;
ring-buffer bounds; registration debounce; redaction; and the two validation
bugs (#8, #6) as regression tests.

**Server (50 tests, in CI):** PKCE against the RFC 7636 worked example, redirect
allow-listing, page XSS escaping, the pending-map cap, and that the approval page
calls the API relative to its own prefix.

**Swift:** `swift build` in `agent-app.yml`, on pull requests too.

**Manual QA on a clean Mac before each release:** fresh DMG install → sign in →
tool call from Claude Desktop; kill a backend process and watch it recover; add
and remove a backend without a restart; close the window and reopen from the menu
bar; reboot with start-at-login (no window, no focus steal); update from N-1 via
the updater; migrate a pre-existing `config.toml` + old LaunchAgent.

---

## 13. Migration for existing users

On first launch the app reads the existing `~/.mcp-gateway-agent/config.toml`
unchanged, moves the plaintext API key into the Keychain and rewrites the file
without it, and leaves the backends exactly as they were. The old
`com.mcpgateway.agent` LaunchAgent and the
`~/.mcp-gateway-agent/bin/mcp-gateway-agent` binary can be removed by hand; the
app does not touch them.

The Keychain migration only runs when the Keychain has nothing, so it can never
clobber a key from a later sign-in.

Ordering matters: **publish the new app first, then delete the old releases** —
deleting first would strand anyone still on the TUI with a self-update that 404s.

---

## 14. Status

| Phase | Deliverable | State |
|---|---|---|
| 0 | Decisions D1–D5; icons | Done |
| 1 | `mcp-gateway-agent-core` with all eleven fixes + tests | Done — 66 tests |
| 2 | `mcp-gateway-agent-ffi`, the C ABI | Done — 6 tests |
| 3 | SwiftUI app: shell, tray, sign-in, all seven pages, Preferences, Keychain, autostart | Done |
| 4 | Server: `backend` filters, `/audit/stats` scoping, OAuth sign-in | Done — 50 tests |
| 5 | `build.sh`, DMG, updater, `agent-app.yml`, `agent-latest` | Done — a signed `.app` builds locally and in CI |
| 6 | Delete TUI/CLI/installers/Docker rows/`agent_releases`; docs | Done |
| 7 | QA checklist on a clean Mac; delete the old releases | **Outstanding** |

---

## 15. Decisions

- **D1 — Signing: ad-hoc, no Apple Developer Program.** The DMG is ad-hoc signed.
  First install needs right-click → Open (or `xattr -dr com.apple.quarantine`),
  documented in the README, `docs/agent.md`, and the release notes. Auto-updates
  after that are unaffected — the updater replaces the bundle directly and no
  quarantine attribute is applied. The workflow is written so that adding
  `APPLE_CERTIFICATE` / `APPLE_SIGNING_IDENTITY` / `APPLE_ID` / `APPLE_PASSWORD`
  / `APPLE_TEAM_ID` later switches on notarization with no rewrite.
  One consequence worth knowing: keychain access is granted to a *signature*, and
  an ad-hoc signature differs in every build, so macOS may re-prompt for
  Keychain access after an update. "Always Allow" settles it.
- **D2 — Stop publishing the agent Docker image.** `mcp-gateway-agent/Dockerfile`
  and the agent rows in the `build` / `merge` / `prune` matrices are gone.
  Existing `sidpad03/mcp-gateway-agent` tags are left on Docker Hub (the prune
  job no longer touches that repo); the README notes it is no longer published.
- **D3 — Remove `/api/v1/agent/releases*`.** `agent_releases.rs`, its `merge`
  line, `agent_release_cache` from `AppState`, and the `RELEASE_PROXY_*` /
  `GITEA_*` variables from the docs. `semver` is still used by `api/updates.rs`,
  so it stays a dependency.
- **D4 — Audit/Usage scope: the agent's own API key.** No auth or schema change.
  The app shows what that account would see in the dashboard, narrowed to this
  machine. Both pages carry an explicit scope line — "Showing all users' calls to
  this machine" (owner key) or "Showing your calls to this machine" — so the
  number on screen is never ambiguous.
- **D5 — Sign in through the browser; no key handling in the dashboard.** The
  Add Agent modal minted a key and asked the user to copy it into a config file.
  That flow is replaced by §8a and deleted from the dashboard. A Mac authorizes
  itself.

---

## 16. If cross-platform becomes a goal

The `core` / `ffi` split means a second platform is a new UI, not a new agent.
The order of preference, recorded now so the reasoning is not re-derived later:

1. **A headless binary.** If "Linux support" means a server rather than a
   desktop, that is a `main.rs` over the existing core — roughly a hundred lines
   — and no UI project at all.
2. **Tauri** for a Windows/Linux GUI. It links the same staticlib directly, ships
   at ~12 MB, and would reuse the dashboard's design system in a webview. The
   cost is that Liquid Glass becomes the CSS imitation described in §2.
3. **Electron** only if the ecosystem is worth ~150 MB and dragging the Rust core
   out into a sidecar process. For an app this size, it is not.
