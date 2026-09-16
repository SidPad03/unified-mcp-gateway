# Homepage widget

Show the gateway on a [Homepage](https://gethomepage.dev) dashboard: how many
tools it serves, how many backends it has, and how many calls it carried in the
last 24 hours.

It uses Homepage's built-in [`customapi`](https://gethomepage.dev/widgets/services/customapi/)
widget, so there is nothing to install on the Homepage side. The gateway serves a
flat summary at `GET /api/v1/stats`, and the widget reads it with a **stats
token** that can do nothing else.

## Set it up

1. Sign in to the dashboard as an owner and open **Settings → Homepage widget**.
2. Choose **Create token**. It is shown once; the gateway keeps only a hash.
3. Set **Address Homepage uses to reach the gateway** (see below), and pick the
   fields you want on the tile.
4. Choose **Copy YAML** and paste the entry under one of the groups in Homepage's
   `services.yaml`.

The generated entry looks like this:

```yaml
- MCP Gateway:
    href: https://mcp-gateway.example.com
    description: MCP tools behind the gateway
    icon: https://raw.githubusercontent.com/SidPad03/unified-mcp-gateway/main/mcp-gateway.svg
    widget:
      type: customapi
      url: https://mcp-gateway.example.com/api/v1/stats
      refreshInterval: 60000
      headers:
        Authorization: Bearer {{HOMEPAGE_VAR_MCPGW_TOKEN}}
      mappings:
        - field: tools
          label: Tools
          format: number
        - field: backends
          label: Backends
          format: number
        - field: calls_24h
          label: Calls (24h)
          format: number
```

### Keep the token out of the file

Homepage substitutes environment variables that start with `HOMEPAGE_VAR_`. Put
the token on the Homepage container and reference it, as above:

```yaml
# Homepage's docker-compose.yml
services:
  homepage:
    environment:
      HOMEPAGE_VAR_MCPGW_TOKEN: mcpgw_stats…
```

### Which address to use

Homepage requests the widget **from its own server**, not from your browser, so
`url` must be reachable from wherever Homepage runs. `href` is the link on the
tile, which your browser follows.

| Homepage runs… | `url` |
|----------------|-------|
| Anywhere that reaches the gateway's public address | `https://mcp-gateway.example.com/api/v1/stats` |
| In the same Docker network as this repository's `docker-compose.yml` | `http://mcp-gateway-dashboard/api/v1/stats` (nginx proxies `/api/`), or `http://mcp-gateway-server:3200/api/v1/stats` |
| On the Docker host, gateway on the default ports | `http://<host-ip>:8080/api/v1/stats` |

## Fields

Every figure is a top-level key, so a mapping names it directly.

| Field | Format | What it counts |
|-------|--------|----------------|
| `tools` | `number` | Tools a client can call now: enabled, on an enabled backend. The Backends page's "Tools behind the gate" |
| `tools_registered` | `number` | Every tool the backends published, disabled ones included |
| `backends` | `number` | Every registered backend |
| `backends_enabled` | `number` | Backends that are switched on |
| `backends_healthy` | `number` | Enabled and answering |
| `backends_unhealthy` | `number` | Enabled and not answering — the Backends page's "Needs attention". A backend that has simply not been started (`idle`) is not counted |
| `agents_connected` | `number` | Macs whose agent is connected |
| `calls_24h` | `number` | Calls in the audit trail in the last 24 hours, whatever their outcome |
| `errors_24h` | `number` | Calls that failed, including a tool that answered with `isError` |
| `denied_24h` | `number` | Calls a policy refused |
| `error_rate_24h` | `percent` with `scale: 100` | `errors_24h / calls_24h` as a fraction; Homepage's `percent` expects a percentage |
| `avg_latency_ms_24h` | `float` with `suffix: " ms"` | Mean call duration in the last 24 hours |
| `last_call_at` | `relativeDate` | The newest audit row, RFC 3339; `null` when the trail is empty |
| `version` | `text` | The gateway's version |

None of them include the gateway's own `gateway_*` and `agent_*` control tools,
or their calls, which is true of every count the dashboard shows.

Homepage's default block view shows **the first four** mappings and drops the
rest. With more than four, add `display: list` — the Settings card does this for
you.

## The stats token

A homelab dashboard keeps its credentials in a YAML file, and the key you would
otherwise paste there — an owner's `mcpgw_` API key — can call every tool behind
the gateway and reconfigure the gateway itself. The stats token cannot:

- It opens `GET /api/v1/stats` and nothing else. It is not an API key, so every
  other endpoint, `/mcp` included, answers `401`.
- There is one at a time. **Regenerate** replaces it and the old one stops
  working immediately; **Revoke** removes it.
- The gateway stores its SHA-256, never the token, and the Settings card shows
  when it was last used (to the minute).
- It starts `mcpgw_stats`, so the audit redactor removes it from any payload it
  turns up in, as it does an API key.

An owner's own session or API key can also read `/api/v1/stats`, which is what
the Settings card's preview uses. Anyone else gets `403`: these are
deployment-wide figures, gated like `/api/v1/metrics/summary`.

## Troubleshooting

| The tile shows | Check |
|----------------|-------|
| `API Error` / `401` | The token was regenerated or revoked, or `Authorization` is missing the `Bearer ` prefix. `curl -H "Authorization: Bearer $TOKEN" <url>` from the Homepage host shows the gateway's answer |
| `API Error` / `403` | The request is using a non-owner's key instead of the stats token |
| `API Error` with no status | Homepage cannot reach `url` — see [Which address to use](#which-address-to-use) |
| The dashboard's HTML instead of numbers | `url` is missing `/api/v1/stats`, so the SPA answered |
| Only four fields | Add `display: list` |

## Other dashboards

Nothing here is specific to Homepage. Homarr, Dashy, Glance, Home Assistant's
REST sensor and a shell script can all read the same endpoint:

```bash
curl -s -H "Authorization: Bearer $MCPGW_STATS_TOKEN" \
  https://mcp-gateway.example.com/api/v1/stats | jq '{tools, backends, calls_24h}'
```
