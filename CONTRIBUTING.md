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

- **Rust** (latest stable) for the server and agent
- **Node.js** (18+) and npm for the dashboard
- **PostgreSQL 16** (or use the included docker-compose)
- **Docker** for containerized development

### Running Locally

```bash
# Start PostgreSQL
docker compose up postgres -d

# Run the server
cd mcp-gateway-server
cargo run

# Run the dashboard (in another terminal)
cd mcp-gateway-dashboard
npm install
npm run dev

# Run the agent (in another terminal)
cd mcp-gateway-agent
cargo run -- setup
cargo run -- run
```

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
# …and the same three for mcp-gateway-agent

# Dashboard
cd mcp-gateway-dashboard && npm ci && npx tsc --noEmit && npm run build
```

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

See the [README](README.md) for an overview of the three-component architecture (server, dashboard, agent).

## License

By contributing, you agree that your contributions will be licensed under the Apache 2.0 License.
