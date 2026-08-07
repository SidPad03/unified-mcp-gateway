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

Tags are `:latest` and `:v<version>` (e.g. `:v1.0.0`). Pin to a version tag for
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
image: sidpad03/mcp-gateway-server:v1.0.0
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

## Moving a deployment

`pg_dump` moves the rows, and moving `.env` with them moves `JWT_SECRET` — which
matters, because each stored API key is encrypted under `SHA-256(JWT_SECRET)`.
Restore the dump on the target with the *same* secret and the keys keep working
and stay revealable; restore it under a different secret and they still
authenticate (the hash is host-independent) but can no longer be revealed in the
dashboard.

```bash
# on the source
docker compose exec -T postgres pg_dump -U mcpgw mcpgw > mcpgw.sql
cp .env mcpgw.env          # keep JWT_SECRET with the dump

# on the target
docker compose up -d postgres
docker compose exec -T postgres psql -U mcpgw mcpgw < mcpgw.sql
docker compose up -d
```

Treat both files as credentials: the dump contains password hashes, backend
configs (including any bearer tokens), and API keys.

## Upgrading

The dashboard tells you when a release is available. An **Update available**
notice appears in the sidebar footer, and **Settings → About** has the running
version, the release notes and a **Check for updates** button. It compares the
running build against the newest `gateway-v*` GitHub release.

The check goes through the server rather than the browser — the dashboard's CSP
blocks direct calls to `api.github.com`, and proxying means one cached lookup
serves every operator instead of each browser spending from GitHub's
unauthenticated rate limit. The server caches that lookup for 30 minutes, and
the dashboard asks again at most every six hours; the button forces past both.
The notice is shown to owners only, since `/settings` is an owner route.

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
