# Deployment Guide

The server and dashboard ship as prebuilt multi-arch images, so a deployment is
a compose file plus a `.env`.

---

## Images

Every build publishes a multi-arch manifest (`linux/amd64` + `linux/arm64`) to
Docker Hub:

| Component | Image |
|-----------|-------|
| Server | [`sidpad03/mcp-gateway-server`](https://hub.docker.com/r/sidpad03/mcp-gateway-server) |
| Dashboard | [`sidpad03/mcp-gateway-dashboard`](https://hub.docker.com/r/sidpad03/mcp-gateway-dashboard) |
| Agent | [`sidpad03/mcp-gateway-agent`](https://hub.docker.com/r/sidpad03/mcp-gateway-agent) |

Tags are `:latest` and `:v<version>` (e.g. `:v1.1.6`). Pin to a version tag for
reproducible deploys.

> **Retention.** CI prunes Docker Hub after each publish, keeping `latest` and
> the **five most recent** version tags per image. Pin only to a version still
> within that window, or mirror the image into your own registry if you need to
> hold a release longer.

---

## Quick deployment

```bash
# Get the compose file
curl -O https://raw.githubusercontent.com/SidPad03/unified-mcp-gateway/main/docker-compose.yml

# A JWT secret is required — the server refuses to boot without one
echo "JWT_SECRET=$(openssl rand -hex 32)" > .env

docker compose up -d
```

This brings up three containers:

| Service | Port | Notes |
|---------|------|-------|
| `mcp-gateway-server` | 3200 | REST API, MCP endpoint, agent WebSocket |
| `mcp-gateway-dashboard` | 8080 | nginx serving the built dashboard |
| `postgres` | — | Internal to the compose network; data in the `postgres_data` volume |

Open `http://localhost:8080` and log in as `admin` / `admin`. You will be
required to set a new password before you can use the dashboard.

---

## Production `.env`

```env
# Required — the server will not start without it
JWT_SECRET=your-strong-random-secret

# Strongly recommended — defaults to `mcpgateway` otherwise
POSTGRES_PASSWORD=your-strong-db-password

# Optional — if unset, admin/admin with a forced change on first login
MCPGW_ADMIN_PASSWORD=your-initial-admin-password
```

Keep `.env` out of version control — it is git-ignored by default.

### Pinning a release

Change the `image:` tags in `docker-compose.yml`:

```yaml
image: sidpad03/mcp-gateway-server:v1.1.3
```

Upgrading is then `docker compose pull && docker compose up -d`. Migrations run
automatically at boot.

---

## Reverse proxy and TLS

**Always** run behind a TLS-terminating reverse proxy in production. The
gateway carries API keys and JWTs in `Authorization` headers, and agents connect
over `wss://`.

An nginx sketch:

```nginx
server {
    listen 443 ssl http2;
    server_name mcp-gateway.example.com;

    ssl_certificate     /etc/ssl/certs/your-cert.pem;
    ssl_certificate_key /etc/ssl/private/your-key.pem;

    # Dashboard
    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-Proto $scheme;
    }

    # API + MCP endpoint
    location ~ ^/(api|mcp)/ {
        proxy_pass http://127.0.0.1:3200;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-Proto $scheme;
    }

    # Agent WebSocket — needs upgrade headers and a long read timeout
    location /agent/ws {
        proxy_pass http://127.0.0.1:3200;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_read_timeout 3600s;
    }
}
```

Do **not** proxy `/metrics` to the internet — it is unauthenticated. Leave it
reachable only from your monitoring network.

---

## Connecting a client

Generate an API key from the dashboard's **Settings** page, then point your MCP
client at the gateway:

```jsonc
// Claude Desktop — ~/.claude/claude_desktop_config.json
{
  "mcpServers": {
    "gateway": {
      "url": "https://mcp-gateway.example.com/mcp",
      "headers": { "Authorization": "Bearer mcpgw_your_key" }
    }
  }
}
```

All backends' tools are now available through that single endpoint.

---

## Building from source

```bash
git clone https://github.com/SidPad03/unified-mcp-gateway.git
cd unified-mcp-gateway
docker compose up --build
```

Or run the pieces directly — see [Development](../README.md#development).

---

## Backup and restore

The only stateful component is PostgreSQL.

```bash
# Back up
docker compose exec -T postgres pg_dump -U mcpgateway mcpgateway > backup.sql

# Restore
docker compose exec -T postgres psql -U mcpgateway mcpgateway < backup.sql
```

Back up your `.env` too. Losing `JWT_SECRET` invalidates every issued session
token; losing the database loses users, backends, policies, and the audit trail.

---

## Moving a deployment (configuration transfer)

A `pg_dump` moves the rows, but it does **not** move working API keys: each
stored key is encrypted under `SHA-256(JWT_SECRET)`, so on a host with a
different secret the dashboard can no longer reveal them. Use the built-in
transfer instead — it handles that re-encryption.

**Settings → Configuration transfer** (owner-only).

### Export

1. Choose an encryption passphrase (12 characters minimum).
2. Decide whether to include audit history — it is usually most of the bundle.
3. Download the `.mcpgw.json` file.

The bundle is gzipped JSON sealed with ChaCha20-Poly1305 under an Argon2id key
derived from your passphrase. It contains everything: users and password hashes,
roles, policies, backends and their configs (including any bearer tokens), tools,
API keys, and optionally the audit log.

> **There is no recovery for a lost passphrase.** The server never stores it.
> Keep it with the file — and treat the file itself as a credential, because it
> is one.

### Import

1. On the **target** deployment, open **Settings → Configuration transfer**.
2. Select the bundle, enter its passphrase, and type `REPLACE` to confirm.

Import **replaces everything**. Every user, key, backend, policy, tool, and audit
event already on the target is deleted first, so the result matches the bundle
exactly rather than merging into whatever was there. It runs in a single
transaction — if any row fails, nothing changes.

Afterwards, sign out and back in: your session belonged to the data that was
just replaced.

### What survives

| | Result |
|---|---|
| Users, roles, policies, backends, tools | Restored exactly, same UUIDs |
| Audit history | Restored if the bundle included it |
| API keys | Keep working **and** stay revealable — re-encrypted under the target's `JWT_SECRET` on import |
| Keys created before at-rest encryption existed | Still authenticate, but cannot be revealed again |

The source and target do **not** need the same `JWT_SECRET`. Existing dashboard
sessions on the target are invalidated, since the user rows behind them changed.

---

## Upgrading

The dashboard tells you when a release is available: **Settings → About →
Check for updates**. It compares the running build against the newest
`gateway-v*` GitHub release.

The check runs only when you click it, and goes through the server rather than
the browser — the dashboard's CSP blocks direct calls to `api.github.com`, and
proxying means one cached lookup serves every operator instead of each browser
spending from GitHub's unauthenticated rate limit.

| Variable | Effect |
|----------|--------|
| `UPDATE_CHECK_REPO` | Check a different repo (default `SidPad03/unified-mcp-gateway`) |
| `UPDATE_CHECK_DISABLED` | Set to anything to disable the check — for air-gapped deployments |
| `GITHUB_TOKEN` | Optional; raises GitHub's rate limit for the check |

If the check cannot reach GitHub it says so explicitly rather than reporting
"up to date", so a stale deployment is never mistaken for a current one.

To upgrade:

1. Read [CHANGELOG.md](../CHANGELOG.md) for the target version.
2. Back up the database.
3. `docker compose pull && docker compose up -d`.

Migrations run at boot. Rolling back an image does **not** roll back a
migration, so restore from backup if you need to go backwards across one.

---

## Troubleshooting

| Symptom | Cause |
|---------|-------|
| Server exits immediately with a `JWT_SECRET` message | Unset, too short, or left at the dev default |
| Login succeeds but every other call returns 403 | The account owes a forced first-login password change; complete it in the dashboard. Login itself returns 200 with `must_change_password: true` |
| Backend shows `unhealthy` | Discovery failed — check the backend URL/command and the server logs, then re-run **Sync** |
| Agent backend shows `disconnected` | No agent is currently connected under that `agent_id` |
| Dashboard loads but every call 401s | Session expired; the login page will say so |

Server logs: `docker compose logs -f mcp-gateway-server`.
