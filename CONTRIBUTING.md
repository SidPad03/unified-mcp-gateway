# Contributing to MCP Gateway

Thank you for your interest in contributing! This document provides guidelines for contributing to the project.

## Getting Started

1. Fork the repository
2. Clone your fork locally
3. Create a feature branch: `git checkout -b my-feature`
4. Make your changes
5. Test locally with `docker compose up --build`
6. Push and open a pull request

## Development Setup

### Prerequisites

- **Rust** (latest stable) for the server and the agent core
- **Node.js** (18+) and npm for the dashboard
- **PostgreSQL 16** (or use the included docker-compose)
- **Docker** for containerized development
- **macOS 26 + Command Line Tools** to build the agent app. Full Xcode is not
  required — the CLT ship the macOS 26 SDK, SwiftUI, `iconutil`, `codesign`
  and `hdiutil`, which is everything `macos/build.sh` uses. The agent's Rust
  crates build and test on any platform.

### Running Locally

```bash
# Start PostgreSQL. The compose `postgres` service publishes no host port — it
# is reachable only from inside the compose network — so for running the server
# on the host, start one with a published port instead:
docker run -d --name mcpgw-dev-pg -p 5432:5432 \
  -e POSTGRES_USER=mcpgateway -e POSTGRES_PASSWORD=mcpgateway \
  -e POSTGRES_DB=mcpgateway postgres:16-alpine

# Run the server. JWT_SECRET is required and must be at least 16 characters —
# the server refuses to boot without it, and reads it from the environment
# rather than from a .env file. DATABASE_URL defaults to the instance above.
cd mcp-gateway-server
JWT_SECRET=$(openssl rand -hex 32) cargo run

# Run the dashboard (in another terminal)
cd mcp-gateway-dashboard
npm install
npm run dev

# Agent core + C ABI: no macOS required
cd mcp-gateway-agent
cargo test --workspace

# The agent app (macOS only). Rebuild and relaunch after a change:
./macos/build.sh && open "build/MCP Gateway Agent.app"
```

`build.sh` takes `--universal` (both architectures — needs rustup targets),
`--dmg`, and `--debug`. Without a signing identity it signs ad-hoc, which is
what CI does too.

## Code Style

- **Rust**: `cargo fmt` and `cargo clippy` are **enforced in CI** — formatting
  differences and any clippy warning fail the build.
- **TypeScript/React**: Follow the existing patterns in the dashboard codebase.
  `tsc --noEmit` must be clean.

## Pull Requests

`main` is protected: it takes no direct pushes, and every change lands through a
pull request with a green `test` check.

```bash
git checkout -b feat/my-change
# …work…
git push -u origin feat/my-change
gh pr create
```

- Keep PRs focused on a single change
- Include a clear description of what changed and why
- Add tests for new functionality where applicable
- Mechanical changes (a formatting sweep, a rename) belong in their own commit so
  reviewers can skip them

## Running the checks locally

Reproduce exactly what CI runs before you push:

```bash
# Rust — both crates
cargo fmt --manifest-path mcp-gateway-server/Cargo.toml --check
cargo clippy --manifest-path mcp-gateway-server/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path mcp-gateway-server/Cargo.toml

cargo fmt --manifest-path mcp-gateway-agent/Cargo.toml --all --check
cargo clippy --manifest-path mcp-gateway-agent/Cargo.toml --workspace --all-targets -- -D warnings
cargo test --manifest-path mcp-gateway-agent/Cargo.toml --workspace

# The agent app (macOS only; CI builds it on every pull request)
cd mcp-gateway-agent && ./macos/build.sh

# Dashboard
cd mcp-gateway-dashboard && npm ci && npx tsc --noEmit && npm run build
```

### Where CI runs

Everything except the macOS agent app runs on the repository's self-hosted
`homelab` runners (`runs-on: [self-hosted, homelab]`). They need Docker — for the
Postgres service container in the `test` job and for buildx in `build` — and
nothing else preinstalled: the workflow installs rustup if it is missing, and
`actions/setup-node` brings its own Node.

Two details worth knowing before editing `ci.yml`:

- **The Postgres service publishes no fixed host port.** It declares the
  container port only, and the test step reads the assigned host port back out of
  `job.services.postgres.ports['5432']`. Pinning `5432:5432` would collide with a
  Postgres already running on the runner host, and with the second runner if both
  picked up this job at once.
- **The runners are x86_64 but the images are multi-arch.** arm64 is
  cross-compiled inside the Dockerfiles rather than emulated. If you add a
  dependency that needs a C toolchain or a system library, the arm64 build is
  where it will break — check `cargo tree --target x86_64-unknown-linux-gnu -i
  <crate>` before assuming a crate is pure Rust, because platform-gated
  dependencies do not show up in a `cargo tree` run on macOS.

`agent-app.yml` is the exception: it needs the macOS 26 SDK, so it runs on
GitHub's `macos-26` image. The agent's Rust crates and their tests still run on
the self-hosted fleet with everything else — only the Swift build is up there.

### Database-backed tests

Some tests exercise real SQL that has no in-process equivalent — the
configuration-transfer round trip depends on Postgres' `row_to_json` and
`jsonb_populate_record`. They **skip silently** unless `TEST_DATABASE_URL` is
set, so a plain `cargo test` passes without a database. CI always sets it, and
you should too when touching that code:

```bash
docker run -d --name mcpgw-test-pg -p 55432:5432 \
  -e POSTGRES_USER=mcpgateway -e POSTGRES_PASSWORD=mcpgateway \
  -e POSTGRES_DB=mcpgateway postgres:16-alpine

TEST_DATABASE_URL=postgresql://mcpgateway:mcpgateway@localhost:55432/mcpgateway \
  cargo test --manifest-path mcp-gateway-server/Cargo.toml
```

These tests **wipe every table** in the target database. Point them at a
throwaway instance, never at anything you care about.

## Architecture

See the [README](README.md) for an overview of the three-component architecture
(server, dashboard, agent), and [docs/agent-desktop-app.md](docs/agent-desktop-app.md)
for the agent app's design and the reasoning behind it.

## License

By contributing, you agree that your contributions will be licensed under the Apache 2.0 License.
