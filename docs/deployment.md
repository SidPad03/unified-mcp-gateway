# Deployment Guide

The server and dashboard ship as prebuilt multi-arch images, so a deployment is
a compose file plus a `.env`.

---

## Images

Every build publishes the same multi-arch manifest (`linux/amd64` +
`linux/arm64`) to both registries:

| Component | Docker Hub (default) | GHCR mirror |
|-----------|---------------------|-------------|
| Server | `sidpad03/mcp-gateway-server` | `ghcr.io/sidpad03/unified-mcp-gateway/mcp-gateway-server` |
| Dashboard | `sidpad03/mcp-gateway-dashboard` | `ghcr.io/sidpad03/unified-mcp-gateway/mcp-gateway-dashboard` |
| Agent | `sidpad03/mcp-gateway-agent` | `ghcr.io/sidpad03/unified-mcp-gateway/mcp-gateway-agent` |

Tags: `:latest`, `:v<version>` (e.g. `:v1.1.3`), and `:<git-sha>`. Pin to a
version tag for reproducible deploys.

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

## Upgrading

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
| Login returns 401 with correct credentials | The account owes a forced first-login password change; complete it in the dashboard |
| Backend shows `unhealthy` | Discovery failed — check the backend URL/command and the server logs, then re-run **Sync** |
| Agent backend shows `disconnected` | No agent is currently connected under that `agent_id` |
| Dashboard loads but every call 401s | Session expired; the login page will say so |

Server logs: `docker compose logs -f mcp-gateway-server`.
