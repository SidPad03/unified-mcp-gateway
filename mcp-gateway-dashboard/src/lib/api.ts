const API_BASE = '/api/v1';

function getToken(): string | null {
  return localStorage.getItem('mcpgw_token');
}

async function request<T>(path: string, options: RequestInit = {}): Promise<T> {
  const token = getToken();
  const headers: Record<string, string> = {
    'Content-Type': 'application/json',
    ...((options.headers as Record<string, string>) || {}),
  };
  if (token) {
    headers['Authorization'] = `Bearer ${token}`;
  }

  const res = await fetch(`${API_BASE}${path}`, {
    ...options,
    headers,
  });

  // A 401 means the token is missing/invalid/expired — clear the session and
  // bounce to login. Skip this for the login call itself (bad credentials are
  // a 401 the Login page should surface inline, not a reason to hard-reload)
  // and avoid a redirect loop if we're already on /login.
  if (res.status === 401 && path !== '/auth/login') {
    localStorage.removeItem('mcpgw_token');
    localStorage.removeItem('mcpgw_user');
    if (window.location.pathname !== '/login') {
      window.location.href = '/login';
    }
    const err = await res.json().catch(() => ({ error: 'Session expired' }));
    throw new Error(err.error || 'Unauthorized');
  }

  if (!res.ok) {
    const err = await res.json().catch(() => ({ error: 'Request failed' }));
    throw new Error(err.error || `HTTP ${res.status}`);
  }

  return res.json();
}

export const api = {
  // Auth
  login: (username: string, password: string) =>
    request<{ token: string; user: User }>('/auth/login', {
      method: 'POST',
      body: JSON.stringify({ username, password }),
    }),

  // Tools
  getTools: (params?: Record<string, string>) => {
    const qs = params ? '?' + new URLSearchParams(params).toString() : '';
    return request<Tool[]>(`/tools${qs}`);
  },
  updateTool: (id: string, data: Partial<Tool>) =>
    request(`/tools/${id}`, { method: 'PATCH', body: JSON.stringify(data) }),

  // Backends
  getBackends: () => request<Backend[]>('/backends'),
  createBackend: (data: CreateBackendRequest) =>
    request<Backend>('/backends', { method: 'POST', body: JSON.stringify(data) }),
  updateBackend: (id: string, data: Partial<Backend>) =>
    request(`/backends/${id}`, { method: 'PATCH', body: JSON.stringify(data) }),
  deleteBackend: (id: string) =>
    request(`/backends/${id}`, { method: 'DELETE' }),
  syncBackend: (id: string) =>
    request<{ status: string; tools_discovered: number }>(`/backends/${id}/sync`, { method: 'POST' }),

  // Audit
  getAuditEvents: (params?: Record<string, string>) => {
    const qs = params ? '?' + new URLSearchParams(params).toString() : '';
    return request<{ events: AuditEvent[]; total: number }>(`/audit${qs}`);
  },
  getAuditStats: () => request<AuditStats>('/audit/stats'),
  clearAudit: () => request<{ status: string }>('/audit', { method: 'DELETE' }),

  // Users
  getUsers: () => request<User[]>('/users'),
  createUser: (data: CreateUserRequest) =>
    request<User>('/users', { method: 'POST', body: JSON.stringify(data) }),
  updateUser: (id: string, data: Partial<UpdateUserRequest>) =>
    request(`/users/${id}`, { method: 'PATCH', body: JSON.stringify(data) }),

  // Users (delete)
  deleteUser: (id: string) =>
    request(`/users/${id}`, { method: 'DELETE' }),

  // Roles
  getRoles: () => request<Role[]>('/roles'),
  createRole: (data: CreateRoleRequest) =>
    request<Role>('/roles', { method: 'POST', body: JSON.stringify(data) }),
  updateRole: (id: string, data: UpdateRoleRequest) =>
    request(`/roles/${id}`, { method: 'PATCH', body: JSON.stringify(data) }),
  deleteRole: (id: string) =>
    request(`/roles/${id}`, { method: 'DELETE' }),
  getRoleImpact: (id: string) =>
    request<RoleImpact>(`/roles/${id}/impact`),

  // Policies
  getPolicies: () => request<Policy[]>('/policies'),
  createPolicy: (data: CreatePolicyRequest) =>
    request<Policy>('/policies', { method: 'POST', body: JSON.stringify(data) }),
  updatePolicy: (id: string, data: UpdatePolicyRequest) =>
    request(`/policies/${id}`, { method: 'PUT', body: JSON.stringify(data) }),
  deletePolicy: (id: string) =>
    request(`/policies/${id}`, { method: 'DELETE' }),

  // Metrics
  getMetricsSummary: () => request<MetricsSummary>('/metrics/summary'),

  // API Keys
  getApiKeys: () => request<ApiKey[]>('/api-keys'),
  createApiKey: (data: CreateApiKeyRequest) =>
    request<CreateApiKeyResponse>('/api-keys', { method: 'POST', body: JSON.stringify(data) }),
  deleteApiKey: (id: string) =>
    request(`/api-keys/${id}`, { method: 'DELETE' }),
  updateApiKey: (id: string, data: { name: string }) =>
    request(`/api-keys/${id}`, { method: 'PATCH', body: JSON.stringify(data) }),
  provisionAppKeys: (userId: string) =>
    request<CreateApiKeyResponse[]>(`/api-keys/provision/${userId}`, { method: 'POST' }),
  getKeysByUser: (userId: string) =>
    request<ApiKey[]>(`/api-keys/by-user/${userId}`),
  // Reveal a user's per-app keys in full (self or admin) so a ready-to-paste
  // client config can be built. May rotate legacy hash-only keys.
  revealAppKeys: (userId: string) =>
    request<RevealedKey[]>(`/api-keys/reveal/${userId}`, { method: 'POST' }),
  // Revoke + regenerate the key for one AI client (self or admin), returning the
  // new key in full. Creates it if none exists. One key per (user, application).
  rotateAppKey: (application: string, userId?: string) =>
    request<CreateApiKeyResponse>('/api-keys/rotate', { method: 'POST', body: JSON.stringify({ application, user_id: userId }) }),

  // Usage
  getUsageGraph: (userId?: string, range?: string) => {
    const params: Record<string, string> = {};
    if (userId) params.user_id = userId;
    if (range) params.range = range;
    const qs = Object.keys(params).length ? '?' + new URLSearchParams(params).toString() : '';
    return request<UsageGraph>(`/usage/graph${qs}`);
  },
  getConnections: (userId?: string) => {
    const qs = userId ? `?user_id=${userId}` : '';
    return request<ConnectionStatus[]>(`/usage/connections${qs}`);
  },
};

// Types
export interface User {
  user_id: string;
  username: string;
  email?: string;
  is_active: boolean;
  created_at: string;
  last_login?: string;
  roles: string[];
  must_change_password?: boolean;
}

export interface Tool {
  tool_id: string;
  tool_name: string;
  backend_name: string;
  original_name: string;
  description?: string;
  input_schema?: Record<string, unknown>;
  risk_category?: string;
  is_enabled: boolean;
  last_seen: string;
  call_count_24h: number;
}

export interface Backend {
  backend_id: string;
  name: string;
  transport: string;
  config: Record<string, unknown>;
  risk_category?: string;
  is_enabled: boolean;
  health_status: string;
  last_health_check?: string;
  created_at: string;
  tool_count: number;
}

export interface AuditEvent {
  event_id: string;
  timestamp: string;
  trace_id: string;
  session_id?: string;
  user_id?: string;
  client_id?: string;
  tool_name: string;
  backend_name: string;
  risk_category?: string;
  request_hash?: string;
  response_hash?: string;
  duration_ms?: number;
  status: string;
  error_message?: string;
  policy_decision?: string;
  policy_id?: string;
  risk_flags: string[];
  metadata: Record<string, unknown>;
  application?: string;
}

export interface AuditStats {
  total_events: number;
  events_24h: number;
  success_count: number;
  error_count: number;
  denied_count: number;
  avg_duration_ms: number;
  top_tools: { tool_name: string; count: number }[];
  status_breakdown: { status: string; count: number }[];
  hourly_volume: { hour: string; count: number }[];
}

export interface Policy {
  policy_id: string;
  name: string;
  priority: number;
  tool_pattern: string;
  decision: string;
  reason?: string;
  is_active: boolean;
  risk_categories: string[];
  created_at: string;
  updated_at: string;
  role_ids: string[];
  role_names: string[];
  application_match?: string;
}

export interface RolePolicyInfo {
  policy_id: string;
  name: string;
  tool_pattern: string;
  decision: string;
}

export interface Role {
  role_id: string;
  name: string;
  description?: string;
  is_system: boolean;
  default_policy: string;
  user_count: number;
  policies: RolePolicyInfo[];
}

export interface MetricsSummary {
  total_tool_calls: number;
  calls_last_24h: number;
  active_backends: number;
  total_backends: number;
  total_tools: number;
  enabled_tools: number;
  total_users: number;
  active_policies: number;
  avg_latency_ms: number;
  error_rate: number;
  top_tools_24h: { tool_name: string; call_count: number; avg_duration_ms: number; error_count: number }[];
  backend_health: { name: string; status: string; tool_count: number }[];
  latency_percentiles: { p50: number; p95: number; p99: number };
  calls_by_risk: { risk_category: string; count: number }[];
  hourly_volume: { hour: string; count: number }[];
}

export interface CreateBackendRequest {
  name: string;
  transport: string;
  config: Record<string, unknown>;
  risk_category?: string;
}

export interface CreateUserRequest {
  username: string;
  password: string;
  email?: string;
  role?: string;
}

export interface UpdateUserRequest {
  email?: string;
  is_active?: boolean;
  role?: string;
  password?: string;
}

export interface CreateRoleRequest {
  name: string;
  description?: string;
  default_policy?: string;
}

export interface UpdateRoleRequest {
  name?: string;
  description?: string;
  default_policy?: string;
}

export interface CreatePolicyRequest {
  name: string;
  tool_pattern: string;
  decision: string;
  reason?: string;
  role_ids?: string[];
  risk_categories?: string[];
  application_match?: string;
}

export interface UpdatePolicyRequest {
  name?: string;
  tool_pattern?: string;
  priority?: number;
  decision?: string;
  reason?: string;
  is_active?: boolean;
  role_ids?: string[];
  risk_categories?: string[];
  application_match?: string;
}

export interface ApiKey {
  key_id: string;
  key_prefix: string;
  name: string;
  user_id: string;
  username: string;
  is_active: boolean;
  created_at: string;
  last_used?: string;
  expires_at?: string;
  application?: string;
}

export interface RoleImpact {
  role_name: string;
  is_system: boolean;
  affected_user_count: number;
  affected_users: string[];
  orphaned_users: string[];
  policy_binding_count: number;
}

export interface CreateApiKeyRequest {
  name: string;
  user_id?: string;
  application?: string;
}

export interface CreateApiKeyResponse {
  key_id: string;
  raw_key: string;
  key_prefix: string;
  name: string;
  user_id: string;
  application?: string;
}

export interface RevealedKey {
  key_id: string;
  application: string | null;
  key_prefix: string;
  raw_key: string;
}

export interface UserNode {
  user_id: string;
  username: string;
  call_count: number;
  last_seen?: string;
}

export interface AppNode {
  application: string;
  is_connected: boolean;
  last_seen?: string;
  call_count: number;
}

export interface BackendNode {
  backend_name: string;
  transport: string;
  health_status: string;
  tool_count: number;
}

export interface ToolNode {
  tool_name: string;
  backend_name: string;
  risk_category?: string;
  call_count: number;
  last_call?: string;
}

export interface GraphEdge {
  source: string;
  target: string;
  call_count: number;
  last_call?: string;
}

export interface UsageGraph {
  users: UserNode[];
  applications: AppNode[];
  backends: BackendNode[];
  tools: ToolNode[];
  user_to_app: GraphEdge[];
  app_to_backend: GraphEdge[];
  backend_to_tool: GraphEdge[];
}

export interface ConnectionStatus {
  application: string;
  is_connected: boolean;
  last_seen?: string;
}
