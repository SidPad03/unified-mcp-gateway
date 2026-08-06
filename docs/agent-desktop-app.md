# MCP Gateway Agent → macOS Desktop App

> **Status: planned, not yet built.** This is the plan of record for replacing the
> ratatui TUI + CLI with a single macOS application, built and released
> automatically on push to `main`. Until it ships, [MCP Gateway Agent](agent.md)
> and [Agent Architecture](agent-architecture.md) describe what actually exists
> today — the terminal agent.

---

## 1. Goal

One `.app` on a Mac that:

- keeps the machine's local MCP backends connected to the gateway (the current tunnel),
- does everything the TUI + CLI did, without a terminal,
- adds Logs, Audit, Usage (scoped to this machine) and Backend add/remove,
- looks and behaves like `mcp-gateway-dashboard`,
- updates itself from GitHub Releases.

Non-goals for v1: Linux, Windows, App Store distribution, a headless Linux agent.

---

## 2. Stack: Tauri v2 (Rust core + React/TS webview)

| Option | Verdict |
|---|---|
| **Tauri v2** | **Chosen.** `tunnel.rs` / `local_backends.rs` / `config.rs` move over as a library, nearly unchanged. UI is React 19 + Vite 7 + Tailwind v4 + lucide + recharts + @xyflow/react — byte-for-byte the dashboard's stack, so visual parity is close to free. ~12 MB DMG, WKWebView (no bundled Chromium), native tray, first-class DMG bundling and a signed GitHub-served updater. |
| Electron | Rejected. ~150 MB, higher idle RAM, and the Rust tunnel would have to be rewritten in Node or shipped as a sidecar. Directly contradicts "efficient and super fast". |
| SwiftUI | Rejected. Full rewrite of the tunnel + backend manager in Swift, a third toolchain, a macOS-only build for every contributor, and the dashboard design system rebuilt by hand. |
| Local web UI + daemon | Rejected. That is a web app, not an application. |

---

## 3. Process model

**The app owns the tunnel.** No separate daemon, no IPC socket, no version skew.

- Dock icon while a window is open; closing the window keeps the app alive in the menu bar.
- Menu-bar extra: status dot, agent id, Reconnect, Open, Check for Updates, Quit
  (Quit confirms the first time — it stops the machine's backends).
- **Start at login** via `tauri-plugin-autostart` (writes a LaunchAgent). Optional
  `KeepAlive` plist is available if daemon-grade restart-on-crash is wanted;
  default off, because it fights a deliberate Quit.
- Rejected: a headless launchd daemon + UI client. Two binaries, two versions, an
  IPC protocol to design and secure, and a whole class of "daemon not running"
  failures — for a resilience gain that "start at login" already covers. This is
  how Tailscale / Docker Desktop / Ollama ship on macOS.

---

## 4. Repo layout

```
mcp-gateway-agent/
├── Cargo.toml                 # workspace: core + src-tauri
├── core/                      # mcp-gateway-agent-core — pure Rust, no Tauri
│   └── src/{config,protocol,tunnel,backends/*,supervisor,logbuf,state}.rs
├── src-tauri/                 # mcp-gateway-agent — the app
│   ├── src/{main,commands,events,tray,updater,keychain,autostart,uninstall}.rs
│   ├── tauri.conf.json  icons/  Info.plist
├── ui/                        # React + Vite + Tailwind v4
│   └── src/{App,pages/*,components/*,lib/*}
└── package.json               # tauri CLI + ui scripts
```

The `core` split is not cosmetic: a Tauri crate cannot even `cargo check` on Linux
without `libwebkit2gtk`, so keeping the logic in a GUI-free crate lets `ci.yml`
keep running fmt/clippy/test on the existing ubuntu runner, and leaves the door
open to a headless binary later without another rewrite.

**Design tokens are copied, not shared.** `ui/src/index.css` gets the dashboard's
`@theme` block verbatim (`--color-surface: #0f0f17`, `--color-accent: #7c5cfc`, …)
plus the same card/badge/stat recipes. A shared source directory would break the
dashboard's Docker build, whose context is `./mcp-gateway-dashboard` only. A
comment in both files notes they must stay in sync.

---

## 5. Deleted

| Path | Why |
|---|---|
| `src/tui/**` (1 597 lines) | Replaced by the UI |
| `src/cli.rs`, most of `src/main.rs` | No subcommands |
| `src/service/{macos,linux,windows}.rs` | Replaced by the autostart plugin |
| `src/update/mod.rs` | Replaced by the Tauri updater |
| `install.sh`, `install.ps1` | Replaced by the DMG |
| `mcp-gateway-agent/Dockerfile` + its `ci.yml` matrix rows | D2 — no longer published |
| `mcp-gateway-server/src/api/agent_releases.rs` (+ `agent_release_cache`, `RELEASE_PROXY_*` / `GITEA_*`) | D3 — nothing consumes it once the TUI is gone |
| GitHub releases `agent-v1.0.0` … `agent-v1.1.3` and their tags | Requested |

---

## 6. Core refactor — and the ten defects it fixes

Every item below was verified in the current source.

| # | Defect today | Fix |
|---|---|---|
| 1 | `start_all` is sequential and awaits each backend's `initialize` + `tools/list` (30 s and 60 s timeouts). Ten backends can block startup for minutes. | Spawn all backends concurrently; UI paints immediately; each row goes `starting → ready / failed`. |
| 2 | `start_all` returns `Err` on the **first** failing backend, so `run_foreground` exits — one bad command takes the whole agent down. | Per-backend isolation. A failed backend is a row with an error, never a process exit. |
| 3 | stdio children are stored as `_child` and never awaited. A backend that dies is invisible forever. | Supervisor task per backend: watch for exit, surface `crashed`, restart with capped backoff, expose PID/uptime/restart count. |
| 4 | `stderr` is `Stdio::inherit()` — in a GUI app that goes nowhere. | Piped into a per-backend ring buffer feeding the Logs page. |
| 5 | `LocalBackendManager` is `Arc<>` and immutable after `start_all`; backends cannot be added or removed without a restart. | `RwLock` interior state + `add / remove / restart / enable` and a debounced (~500 ms) re-`register` to the gateway, which the server already handles. |
| 6 | The TUI matches a completion to a call **by tool name** (`find(\|r\| r.tool == tool && r.duration_ms.is_none())`) — two concurrent calls to the same tool update the wrong record. | Correlate by `request_id`. |
| 7 | `chrono_now()` computes `secs % 86400` on the UNIX epoch — that is UTC time-of-day shown as local time. | Real local timestamps. |
| 8 | Setup wizard's gateway check is `timeout(...).await.is_ok()` — true whenever the connect *returned*, including connection-refused. It reports a dead gateway as valid. | Check the inner `Result` (the API-key check next to it already does). |
| 9 | The API key sits in plaintext in `config.toml`. | macOS Keychain, migrated on first launch; the webview never receives it. |
| 10 | Sub-backends are registered to the gateway as if all were healthy, even one that failed to start. | Report only backends that actually came up, with their real tool counts. |

Also added to the core:

- **Bounded ring buffers** in Rust (5 000 log lines, 1 000 tool calls) — the source
  of truth, so the webview holds no unbounded history.
- **Event coalescing**: log lines and tool-call events batch on a ~100 ms tick
  instead of one IPC message per line. A chatty backend must not be able to flood
  the webview.
- **Atomic config writes** (temp file + `rename`, `0600`).
- Unchanged: the wire protocol, reconnect/backoff behaviour, JSON-RPC id
  correlation, and the "log argument counts, never arguments" rule.

---

## 7. UI

Shell: the dashboard's 256 px sidebar, `#7c5cfc` accent, `text-[11px] uppercase
tracking-[0.15em]` section headers, lucide icons, Inter — rendered in Liquid
Glass materials rather than flat fills (§7a).

| Page | Contents | Data source |
|---|---|---|
| **Overview** | Connection hero (state, gateway, agent id, uptime), tools registered, backends up/down, 24 h call sparkline, Reconnect / Re-register / Open Dashboard | Local (instant) |
| **Backends** | Status dot, transport, tool count, PID, uptime, last error; Add (stdio/http with env + headers), Edit, Remove, Restart, Enable, **Test connection** before save | Local |
| **Activity** | Live tool calls: time, tool, backend, duration, ok/err, expandable error | Local |
| **Logs** | Agent + per-backend stderr merged; level/backend filters, search, follow-tail, copy, export, Reveal in Finder; secrets redacted with the server's redactor rules | Local |
| **Audit** | Server audit events for this machine, dashboard-style filters + detail drawer | `GET /audit?backend=<agent_id>` |
| **Usage** | `App → Agent → Sub-backend → Tool` graph (@xyflow/react) plus calls-over-time, top tools, latency and error-rate charts (recharts) | `GET /usage/graph?backend=<agent_id>` + local sub-backend grouping |
| **Settings** | Gateway URL, agent id, API key (Keychain, masked), TLS-skip warning toggle, start at login, close-to-menu-bar, update channel + Check for updates, About, config path, Reset/Uninstall | Local |
| **First-run wizard** | Gateway URL → API key (live-validated) → Agent ID → Backends → Done | Local |

Performance rules, enforced in review:

- All lists virtualized. Logs and Audit never render more than a viewport of rows.
- Local state is event-driven; no polling.
- Server-backed pages poll **only while visible and focused**, incrementally
  (`from=<last_seen_ts>`), with `AbortController`, and stop when hidden.
- `@xyflow/react` and `recharts` are `React.lazy` chunks so Overview's first paint
  does not pay for them.
- Rust release profile moves from `opt-level = "z"` to `"s"` with fat LTO — a few
  hundred KB of DMG for lower call latency.

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
| Window background | `windowEffects: { effects: ["sidebar"], state: "followsWindowActiveState", radius: 12 }` + `transparent: true`. A real `NSVisualEffectView` — composited by the OS on the GPU, costing the webview nothing. |
| Title bar | `titleBarStyle: "Overlay"` + `hiddenTitle: true` with a CSS drag region, so traffic lights float over our own toolbar (the Finder/Mail look). |
| Sidebar | The native `sidebar` material behind a transparent CSS column. |
| Cards, popovers, sheets | CSS glass: `background: color-mix(in srgb, var(--glass-tint) 68%, transparent)`, `backdrop-filter: blur(24px) saturate(180%)`, a hairline border, and an inset top edge (`inset 0 1px 0 rgb(255 255 255 / .12)`) for the specular highlight. |
| Menu-bar popover | `hudWindow` material, 16 px radius, no titlebar. |
| Focus / press | A luminous accent rim (`box-shadow`), not a flat colour swap. |
| Corner radii | Concentric: inner radius = outer radius − padding. Cards `16`, nested rows `10`, window `12`. |

**The performance rule — this is the one that matters.** `backdrop-filter` is
expensive in WKWebView and compounds per element. It is allowed on **chrome
only**: sidebar, toolbars, cards, modals, the tray popover. It is **forbidden**
inside virtualized lists (log rows, audit rows, tool-call rows) and on anything
that animates position. Large surfaces use native `windowEffects` vibrancy in
preference to CSS blur, because the OS composites that without a webview repaint.
Skip this rule and Liquid Glass directly contradicts "super fast" the moment
someone scrolls a 5 000-line log.

**Reconciling with the dashboard.** Identity is unchanged — same accent, same
type scale, same status colours, same card rhythm, same section-header
treatment. What changes is only *how a surface is painted*: `bg-surface` becomes
a tinted material. Side by side they read as one product, one native to the Mac
and one to the browser. The shared token file simply gains a `--glass-*` group
the dashboard does not consume.

**Light and dark.** A Mac app that ignores the system appearance looks broken, so
tokens are authored as custom properties under `:root` and
`@media (prefers-color-scheme: light)`, and the native material follows the
system automatically. Dark stays the primary design target.

**Accessibility.** `prefers-reduced-transparency` falls back to opaque surfaces
with a visible border — macOS "Reduce transparency" is a real setting people rely
on. `prefers-reduced-motion` disables the material transitions.

---

## 7b. Logo

The current mark is a lucide `Zap` bolt in `#a855f7` — generic, and it says
nothing about what the product does.

**New mark — "Converging Gate":** three traces converge on a single point and
pass through a diamond threshold, leaving as one beam. It states the product in
one glyph: many MCP servers, one guarded endpoint. Drawn on lucide's 24 grid at
stroke 2 with round caps, so it sits correctly beside the lucide icons already in
both UIs, and it still reads at 16 px. Alternates explored and kept on file: a
chevron gate (cleanest, but reads as a generic "send"), a hexagon portal (muddy
below 24 px), and a shield gate (polished, but the stock security cliché).

**Asset set** in `brand/`:

| File | Use |
|---|---|
| `mcp-gateway-mark.svg` | The glyph, `currentColor`, for both UIs |
| `mcp-gateway-wordmark.svg` | Mark + "MCP Gateway" lockup for the README |
| `agent-app-icon.svg` → `agent-app-icon-1024.png` | Source for `tauri icon`, which generates the full `.icns` set |
| `agent-tray-Template.svg/png` | Menu-bar icon |

Two details that are easy to get wrong:

- The tray icon **must** be a macOS template image — pure black plus alpha, named
  with the `Template` suffix (or flagged `is_template`) — or it will not invert
  on a light menu bar and will not highlight correctly when clicked.
- The Dock icon is a **filled, heavier** cut of the mark on a tinted glass tile.
  A 2 px line glyph scaled to 1024 looks thin and weak next to Apple's icons.

The agent and the gateway share the mark; the agent's tile carries a violet→cyan
gradient so the two are distinguishable in the Dock.

---

## 8. Server changes (small, additive, backward compatible)

Two are required, one is a hygiene fix.

1. **`GET /usage/graph` gains an optional `backend` filter.** Required, not
   optional: the query has `LIMIT 100` on tools ordered by call count **across all
   backends**, so on a busy gateway this machine's tools can be absent entirely.
   Client-side filtering cannot fix that. Filter before the limit.
2. **`GET /audit/stats` gains the same optional `backend` filter**, so the agent
   can show its own 24 h volume, error rate and latency without pulling rows.
3. **`GET /audit/stats` should apply the same non-owner user scoping `/audit`
   already applies.** Today it returns global totals and top tools to any
   authenticated caller. Pre-existing; worth fixing while we are in the file.
   Droppable if you would rather keep this change set minimal.

**Scoping semantics (deliberate):** the app authenticates with the agent's own
`mcpgw_` API key, so it sees exactly what that account would see in the dashboard,
narrowed to this machine. If the key belongs to an owner, that is every user's
calls to this agent; if not, only that user's. The app states which it is showing.
Anything wider would need an agent↔owner link in the schema that does not exist —
see decision D4.

---

## 9. Updates — signed, straight from GitHub

- `tauri-plugin-updater` with a minisign keypair. `TAURI_SIGNING_PRIVATE_KEY` is a
  repo secret; the public key lives in `tauri.conf.json`. Stronger than the old
  SHA-256 check: the signature covers authorship, not just integrity.
- **The endpoint must not be `releases/latest/download/latest.json`.** This repo
  interleaves `gateway-v*` and `agent-v*` tags, so "latest release" is regularly a
  gateway release with no updater manifest, which would break the updater. CI
  instead force-updates a permanent, non-"latest" release tagged **`agent-latest`**
  whose only asset is `latest.json`:

  ```
  https://github.com/SidPad03/unified-mcp-gateway/releases/download/agent-latest/latest.json
  ```

  Stable forever, GitHub-served, no gateway server involved.
- Auto-check on launch + every 6 h; a quiet banner (the dashboard's Settings card
  pattern), release notes, one-click download → install → relaunch.
- **Gatekeeper.** Without Developer ID signing + notarization, a downloaded DMG is
  quarantined and macOS says the app is damaged. In-place updates are unaffected,
  but first install is not. CI signs and notarizes when `APPLE_*` secrets exist and
  falls back to ad-hoc signing with a documented right-click → Open otherwise.
  See decision D1.

---

## 10. CI/CD

**New `.github/workflows/agent-app.yml`** — `push` to `main` on
`mcp-gateway-agent/**`, plus `workflow_dispatch`:

1. `macos-14` runner; Rust with `aarch64-apple-darwin` + `x86_64-apple-darwin`;
   `Swatinem/rust-cache`; Node 20 with npm cache.
2. Compute the version by counting `agent-v*` releases (today's scheme). With the
   every release and tag deleted and all three components reset, the first
   release of the app is **`agent-v1.0.0`**. The earlier `1.1.x` line was
   numbered ahead of where the project actually was; the restart puts the agent,
   the server and the dashboard back on the same `1.0.0` footing.
3. Patch the version into `Cargo.toml` only — `tauri.conf.json` omits `version`
   and inherits it, so the two cannot drift.
4. Optional: import the Apple certificate into a temporary keychain.
5. `tauri-apps/tauri-action` with `--target universal-apple-darwin`: one DMG for
   Apple Silicon and Intel, `createUpdaterArtifacts: true` for the `.app.tar.gz` +
   `.sig`, release created as `agent-v<version>`.
6. Force-update the `agent-latest` release's `latest.json`.

**`ci.yml` changes:**

- Add `mcp-gateway-agent/core` to fmt/clippy/test (ubuntu, no GUI deps needed).
- Add `tsc --noEmit` for `mcp-gateway-agent/ui`.
- ~~Remove `mcp-gateway-agent` from the Docker `build` / `merge` / `prune`
  matrices.~~ **Done.** Left in place it would have republished an image of the
  terminal agent on the very next merge, undoing the release wipe.
- ~~Delete `.github/workflows/agent-release.yml`.~~ **Done**, for the same
  reason: the version reset touches `mcp-gateway-agent/**`, which is exactly
  that workflow's path trigger, so merging would have cut a fresh
  `agent-v1.0.0` release of the TUI.
- The `test` job keeps running fmt / clippy / tests against the agent crate —
  that code still exists until the rewrite lands.

**CodeQL — removed (done).** It was GitHub's *default setup*
(`dynamic/github-code-scanning/codeql`), a repository setting rather than a file
in `.github/workflows/`, which is why it could not be found in the pipeline. Its
runs were failing with `The job was not acquired by Runner of type hosted even
after multiple attempts` on all three language matrices — GitHub failing to
allocate hosted runners, not a finding in the code. Disabled via
`PATCH /repos/{owner}/{repo}/code-scanning/default-setup` → `not-configured`; it
is re-enablable in one click from Settings → Code security if the runner supply
recovers.

No E2E in CI: `tauri-driver` has no macOS support. Replaced by a manual QA
checklist (§12) run against the built DMG.

---

## 11. Docs

`docs/agent.md` rewritten for the app; `docs/agent-architecture.md` keeps the wire
protocol and gains the new process model; `README.md` agent section and component
table ("Rust, ratatui TUI" → "Rust + Tauri, macOS app"); `ARCHITECTURE.md`
diagram line; `CONTRIBUTING.md` dev loop (`npm run tauri dev`); `CHANGELOG.md`
entry noting the breaking change and the removed endpoints; `.env.example` /
`docs/configuration.md` lose `RELEASE_PROXY_*` if D3 is approved.

---

## 12. Testing

**Rust (core, in CI):** config round-trip + Keychain migration; golden-JSON tests
pinning every wire message against the server's enum shapes (the protocol is
duplicated across crates — this is the guard); tool routing; JSON-RPC id
correlation including a server-initiated request mid-stream; supervisor restart
backoff; ring-buffer bounds; registration debounce; the two validation bugs (#8,
#6) as regression tests.

**UI:** `tsc --noEmit` in CI.

**Manual QA on a clean Mac before each release:** fresh DMG install → wizard →
connect → tool call from Claude Desktop; kill a backend process and watch it
recover; add and remove a backend without a restart; quit and relaunch from the
menu bar; reboot with start-at-login; update from N-1 via the updater; migrate a
pre-existing `config.toml` + old LaunchAgent.

---

## 13. Migration for existing users

On first launch the app reads the existing `~/.mcp-gateway-agent/config.toml`
unchanged, offers to move the API key into the Keychain, detects the old
`com.mcpgateway.agent` LaunchAgent and offers to replace it, and offers to remove
the old `~/.mcp-gateway-agent/bin/mcp-gateway-agent` binary and its shell `PATH`
lines. Every step is offered, never forced.

Ordering matters: **publish the new app first, then delete the old releases** —
deleting first would strand anyone still on the TUI with a self-update that 404s.

---

## 14. Phases

| Phase | Deliverable | Checkpoint |
|---|---|---|
| 0 | Decisions D1–D4; updater keypair generated; icons | — |
| 1 | `mcp-gateway-agent-core` with all ten fixes + tests | `cargo test` green; a scratch binary tunnels and hot-reloads |
| 2 | Tauri shell, tray, wizard, Overview, Backends, Settings, Keychain, autostart | End-to-end on your Mac, no terminal |
| 3 | Logs, Activity, Audit, Usage + the server filters | Real data on every page |
| 4 | DMG, signing, updater, `agent-app.yml`, `agent-latest` | Push to `main` produces an installable DMG; N-1 → N update works |
| 5 | Delete TUI/CLI/installers/Docker rows/`agent_releases`; docs; delete old releases | Repo has no terminal agent left |
| 6 | QA checklist on a clean Mac | Ship |

---

## 15. Decisions (locked 2026-08-06)

- **D1 — Signing: ad-hoc, no Apple Developer Program.** The DMG is ad-hoc signed.
  First install needs right-click → Open (or `xattr -dr com.apple.quarantine`),
  documented prominently in the README, `docs/agent.md`, and the release notes
  template. Auto-updates after that are unaffected — the updater replaces the
  bundle directly and no quarantine attribute is applied. The workflow is written
  so that adding `APPLE_CERTIFICATE` / `APPLE_SIGNING_IDENTITY` / `APPLE_ID` /
  `APPLE_PASSWORD` / `APPLE_TEAM_ID` secrets later switches on notarization with
  no rewrite — the cert-import and notarize steps are present but `if:`-gated on
  the secrets existing.
- **D2 — Stop publishing the agent Docker image.** Remove
  `mcp-gateway-agent/Dockerfile` and the agent rows from the `build` / `merge` /
  `prune` matrices in `ci.yml`. README's Docker Hub table drops the agent row.
  Existing `sidpad03/mcp-gateway-agent` tags are left on Docker Hub (the prune job
  no longer touches that repo); the README notes it is no longer published.
- **D3 — Remove `/api/v1/agent/releases*`.** Delete
  `mcp-gateway-server/src/api/agent_releases.rs`, its `merge` line in
  `api/mod.rs`, `agent_release_cache` from `AppState`, and the
  `RELEASE_PROXY_URL` / `RELEASE_PROXY_REPO` / `GITEA_*` variables from
  `.env.example`, `docker-compose.yml`, `docs/configuration.md` and
  `docs/api-reference.md`. `semver` may become an unused server dependency —
  check before removing, `api/updates.rs` still uses it.
- **D4 — Audit/Usage scope: the agent's own API key.** No auth or schema change.
  The app shows what that account would see in the dashboard, narrowed to this
  machine. The Audit and Usage pages carry an explicit scope line — "Showing all
  users' calls to this machine" (owner key) or "Showing your calls to this
  machine" (non-owner key) — so the number on screen is never ambiguous.
