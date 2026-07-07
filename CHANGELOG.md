# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
