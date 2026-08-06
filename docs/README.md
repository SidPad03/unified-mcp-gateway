# MCP Gateway Documentation

MCP Gateway is a self-hosted aggregation, routing, and security layer for the
Model Context Protocol (MCP). It sits between AI clients (Claude Desktop, Cursor,
and other MCP clients) and your MCP tool servers, providing:

- **Single endpoint** — one MCP URL for all your tools
- **Security** — authentication, role-based access control, and policy enforcement
- **Visibility** — a full audit trail of every tool call, with metrics and dashboards
- **Remote access** — connect MCP servers on any machine via the Gateway Agent

## Contents

| Page | What it covers |
|------|----------------|
| [Architecture](architecture.md) | System design, components, request flow, and the trust model |
| [Deployment Guide](deployment.md) | Running the server, dashboard, and database in production |
| [Configuration Reference](configuration.md) | Environment variables, backend transports, and the agent config file |
| [Authentication & Authorization](authentication.md) | JWTs, API keys, roles, and the policy engine |
| [API Reference](api-reference.md) | Every REST endpoint under `/api/v1`, plus the MCP and WebSocket endpoints |
| [MCP Gateway Agent](agent.md) | Installing, configuring, and running the remote agent |
| [Agent Architecture](agent-architecture.md) | The agent↔server WebSocket protocol and connection lifecycle |
| [Agent Desktop App](agent-desktop-app.md) | **Planned** — the macOS app that replaces the terminal agent |

For a condensed overview of the same system design, see
[ARCHITECTURE.md](../ARCHITECTURE.md) at the repository root. For vulnerability
reporting, see [SECURITY.md](../SECURITY.md).

## Quick links

- **Getting started:** [README](../README.md#quick-start)
- **Changelog:** [CHANGELOG.md](../CHANGELOG.md)
- **Contributing:** [CONTRIBUTING.md](../CONTRIBUTING.md)

> Examples in these docs use `https://mcp-gateway.example.com` as the gateway
> address and `mcpgw_your_key` as an API key. Substitute your own.
