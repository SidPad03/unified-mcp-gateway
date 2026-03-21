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

- **Rust**: Follow standard Rust formatting (`cargo fmt`). Run `cargo clippy` before submitting.
- **TypeScript/React**: Follow the existing patterns in the dashboard codebase.

## Pull Requests

- Keep PRs focused on a single change
- Include a clear description of what changed and why
- Ensure existing tests pass
- Add tests for new functionality where applicable

## Architecture

See the [README](README.md) for an overview of the three-component architecture (server, dashboard, agent).

## License

By contributing, you agree that your contributions will be licensed under the Apache 2.0 License.
