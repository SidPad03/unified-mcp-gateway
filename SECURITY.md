# Security Policy

## Reporting Vulnerabilities

If you discover a security vulnerability, please report it responsibly:

1. **Do NOT open a public GitHub issue.**
2. Report it privately through GitHub's
   [security advisory form](https://github.com/SidPad03/unified-mcp-gateway/security/advisories/new).
3. Include steps to reproduce, impact assessment, and any suggested fixes.
4. We will acknowledge receipt within 48 hours and provide a timeline for a fix.

## Security Considerations

### Authentication

- **JWT tokens** carry a dashboard session. `JWT_SECRET` is **required** and has
  no default: the server refuses to boot without it, refuses the well-known
  placeholder, and refuses anything shorter than 16 characters. Generate one
  with `openssl rand -hex 32`.
- **API keys** carry the `mcpgw_` prefix. Each is stored twice: as a SHA-256
  hash, which is what authenticates a request, and as a ChaCha20-Poly1305
  ciphertext under a key derived from `JWT_SECRET`, which is what lets the
  dashboard rebuild a ready-to-paste client config later. A database leak alone
  therefore does not recover a key, but a leak of the database *and*
  `JWT_SECRET` does. `POST /api/v1/api-keys/reveal/{user_id}` returns the
  plaintext to that user or to an owner.
- The seeded `admin` account requires a password change on first login, enforced
  server-side so it cannot be skipped by calling the API directly. Set
  `MCPGW_ADMIN_PASSWORD` to choose the initial password instead.

### Network Security

- Always deploy behind TLS (HTTPS) in production.
- The agent's WebSocket (`/agent/ws`) carries its API key in an `Authorization`
  header. Use `wss://`, not `ws://`.
- The dashboard's live feed (`/api/v1/ws/live`) carries its JWT in the query
  string, because a browser cannot set a header on a WebSocket. Treat proxy
  access logs accordingly, or turn off query-string logging on that path.
- `/metrics` is unauthenticated by design, for Prometheus. Do not expose it to
  the internet; the shipped `docker-compose.yml` publishes it on the server's
  port, so bind that port to your monitoring network rather than to `0.0.0.0`.
- CORS is hard-coded to `http://localhost:8080` and `http://localhost:5173`, the
  two development origins. A production deployment serves the dashboard and the
  API from one origin through a reverse proxy, where CORS does not apply.

### Agent TLS Verification

- The agent supports `tls_skip_verify` for development with self-signed
  certificates. It trusts only the configured gateway host, not any host a
  redirect lands on.
- **Never use `tls_skip_verify = true` in production** as it disables
  certificate validation and enables MITM attacks.

### Data Protection

- Audit payloads and error messages pass through a redactor before they are
  written, covering bearer tokens, labelled credentials, and raw `mcpgw_` keys.
- A stdio backend is third-party code and inherits this process's environment
  minus the variables the gateway reads for itself (`JWT_SECRET`,
  `DATABASE_URL`, `MCPGW_ADMIN_PASSWORD`, `GITHUB_TOKEN`). Everything else in
  the gateway's environment is visible to it.
- Backend environment variables and headers may contain secrets. They are stored
  in the database, are never returned to a non-owner, and are returned to an
  owner as `__mcpgw_masked__` for any key the operator locked.
- Database credentials should use strong passwords and restricted network
  access.

## Supported Versions

| Version | Supported |
|---------|-----------|
| Latest  | Yes       |

## Dependencies

We monitor dependencies for known vulnerabilities. Run `cargo audit` (Rust) and `npm audit` (dashboard) to check for issues.
