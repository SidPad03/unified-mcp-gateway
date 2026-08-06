# Authentication & Authorization

MCP Gateway has two credential types, one privileged role, and a policy engine
that decides whether a given tool call is allowed.

---

## Credentials

### JWT (dashboard sessions)

`POST /api/v1/auth/login` exchanges a username and password for an HS256 JWT.
Passwords are hashed with Argon2. The token carries `sub` (user id) and `roles`.

`JWT_SECRET` is **required** — the server refuses to boot if it is unset, left
at the old development default, or shorter than 16 characters. Because tokens
are signed with it, a known secret would let anyone forge an owner token.
Generate one with `openssl rand -hex 32`.

### API keys (clients and agents)

API keys are `mcpgw_`-prefixed and are what AI clients and remote agents use.
The plaintext is shown **once**, at creation; only a hash is stored. Revoking a
key takes effect immediately — the next request with it gets a 401.

Both credential types resolve to the same `Claims { sub, roles }`. On every
request the server re-checks `is_active` and role membership **against the
database**; it does not trust the token's contents alone. Deactivating a user
therefore takes effect immediately, without waiting for token expiry.

> API keys currently resolve to the full privileges of their owning user. A key
> created by an owner is an owner-level credential — scope key creation to the
> least-privileged account that can do the job.

---

## First boot

The seeded `admin` account is created with the password `admin` and
`must_change_password = TRUE`. That flag is enforced at the auth gate: until the
password is changed, the session can do nothing except change that user's own
password.

Set `MCPGW_ADMIN_PASSWORD` before first boot to choose your own initial password
instead; doing so skips the forced change, on the assumption you picked
deliberately.

---

## Roles

| Role | Capability |
|------|-----------|
| `owner` | Full administrative access. `require_admin` admits this role and no other. |
| everything else | Read and use tools; see only your own audit, usage, and API-key rows. |

Additional roles can be created, but they differ from each other only in the
policies attached to them and their `default_policy` — not in administrative
capability, which is `owner`-only.

Guards worth knowing:

- The last **active** owner cannot be demoted, deactivated, or deleted.
  Inactive owner rows do not count toward that total.
- You cannot delete your own account.

---

## Policy engine

Every tool call is evaluated before it is routed. Rules come from the policies
attached to the caller's roles.

### Rule shape

| Field | Meaning |
|-------|---------|
| `priority` | Lower numbers are evaluated first |
| `tool_pattern` | Comma-separated glob list, e.g. `*__delete_*,*__drop_*` |
| `risk_categories` | Restrict the rule to these risk categories; empty means any |
| `application_match` | Glob against the calling application; empty or absent means any |
| `decision` | `allow` or `deny` |
| `reason` | Recorded on the audit event when this rule decides |

### Evaluation

Rules are sorted by `priority` and the **first match wins**. A rule matches when
the tool pattern matches **and** the risk category matches **and** the
application matches. If no rule matches, the caller's role default applies.

Two behaviors to configure around:

- **Role defaults combine permissively.** When a user holds several roles, the
  effective default is `allow` if *any* of those roles defaults to allow. Assign
  a deny-default role alongside an allow-default one and the allow wins.
- **An application-scoped rule also matches credentials that carry no
  application.** A rule with `application_match` set still applies when the
  caller did not identify an application, so scope rules by tool pattern and
  risk category as your primary control, not by application alone.

Write explicit deny rules for anything you need blocked rather than relying on
defaults.

### Risk classification

Discovered tools are auto-classified into a risk category from their name and
description:

| Category | Meaning |
|----------|---------|
| `read` | Read-only or informational |
| `write` | Creates or modifies resources |
| `admin` | Settings, secrets, permissions |
| `destructive` | Deletes, drops, truncates |
| `execute` | Runs workflows or dispatches actions |
| `unclassified` | Nothing matched |

Admin takes precedence over the verb: `delete_org_action_secret` classifies as
`admin`, not `destructive`, because managing secrets is a governance concern
even when the verb is destructive.

The classifier is best-effort. Override any tool's category via
`PATCH /api/v1/tools/{tool_id}`; overrides survive backend rediscovery.

---

## Audit and redaction

Every call — allowed or denied — is written to `audit_events` with its decision,
the deciding policy, latency, and redacted request/response payloads.

The redactor strips bearer tokens, labeled credentials (`password`, `token`,
`api_key`, `secret`, `authorization` — quoted or bare), raw `mcpgw_` keys, email
addresses, and SSN- and phone-shaped strings before anything is stored.

Audit statuses:

| Status | Meaning |
|--------|---------|
| `success` | The tool returned a normal result |
| `tool_error` | The backend returned `isError: true` — a **failed** call |
| `error` | The gateway could not complete the call, including when the backend timed out |
| `denied` | Policy blocked the call |

`tool_error` counts as an error in `error_count` and in the metrics error rate.

---

## Hardening checklist

- Set a strong `JWT_SECRET` and a strong `POSTGRES_PASSWORD`.
- Change the `admin` password on first login, or preset `MCPGW_ADMIN_PASSWORD`.
- Terminate TLS at a reverse proxy; do not expose the gateway directly.
- Restrict `/metrics` — it is unauthenticated.
- Create per-application API keys rather than sharing one, so a single
  revocation does not disconnect everything.
- Prefer non-owner accounts for day-to-day API keys.
- Leave `tls_skip_verify` off for agents outside a trusted network.
