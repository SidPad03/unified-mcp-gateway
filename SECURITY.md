# Security Policy

## Reporting Vulnerabilities

If you discover a security vulnerability, please report it responsibly:

1. **Do NOT open a public GitHub issue.**
2. Email your findings to the maintainers (see repository contact information).
3. Include steps to reproduce, impact assessment, and any suggested fixes.
4. We will acknowledge receipt within 48 hours and provide a timeline for a fix.

## Security Considerations

### Authentication

- **JWT tokens** are used for session authentication. The `JWT_SECRET` environment variable **must** be set to a strong, random value in production. The default value is only for local development.
- **API keys** use the `mcpgw_` prefix and are stored as SHA-256 hashes in the database. The plaintext key is only shown once at creation time.
- The default admin account (`admin`/`admin`) is created on first startup. **Change the password immediately** in production.

### Network Security

- Always deploy behind TLS (HTTPS) in production.
- The WebSocket agent connection (`/agent/ws`) transmits authentication tokens. Use `wss://` (not `ws://`) in production.
- CORS origins are hardcoded for `localhost` in development. Configure a reverse proxy with appropriate CORS headers for production.

### Agent TLS Verification

- The agent supports `tls_skip_verify` for development with self-signed certificates.
- **Never use `tls_skip_verify = true` in production** as it disables certificate validation and enables MITM attacks.

### Data Protection

- Audit logs include configurable redaction for sensitive data patterns (API keys, tokens, passwords).
- Database credentials should use strong passwords and restricted network access.
- Backend environment variables may contain secrets (API keys for MCP servers). These are stored in the database and should be protected accordingly.

## Supported Versions

| Version | Supported |
|---------|-----------|
| Latest  | Yes       |

## Dependencies

We monitor dependencies for known vulnerabilities. Run `cargo audit` (Rust) and `npm audit` (dashboard) to check for issues.
