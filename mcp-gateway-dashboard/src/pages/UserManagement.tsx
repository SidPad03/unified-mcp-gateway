import { useState, useEffect } from 'react';
import { api, User, Role, ApiKey, RoleImpact } from '@/lib/api';
import { UserPlus, X, Key, Copy, Check, AlertTriangle, Plus, Edit3, Trash2, Shield, Lock } from 'lucide-react';
import clsx from 'clsx';
import { useAuth } from '@/hooks/useAuth';
import { fmt } from '@/lib/format';
import {
  Badge,
  Banner,
  Button,
  Card,
  EmptyState,
  Loading,
  Mono,
  PageHeader,
  Table,
  TableMessage,
  Td,
  Th,
} from '@/components/ui';

export default function UserManagement() {
  const auth = useAuth();
  const [users, setUsers] = useState<User[]>([]);
  const [roles, setRoles] = useState<Role[]>([]);
  const [apiKeys, setApiKeys] = useState<ApiKey[]>([]);
  const [loading, setLoading] = useState(true);
  const [showCreateModal, setShowCreateModal] = useState(false);
  const [showKeyModal, setShowKeyModal] = useState(false);
  const [keyModalUserId, setKeyModalUserId] = useState('');
  const [keyName, setKeyName] = useState('');
  const [generatedKey, setGeneratedKey] = useState('');
  const [copied, setCopied] = useState(false);
  const [newUser, setNewUser] = useState({ username: '', password: '', email: '', role: 'owner' });
  const [showRoleModal, setShowRoleModal] = useState(false);
  const [editingRole, setEditingRole] = useState<Role | null>(null);
  const [roleForm, setRoleForm] = useState({ name: '', description: '', default_policy: 'allow' });
  const [deleteConfirm, setDeleteConfirm] = useState<{ type: 'role' | 'user'; id: string; name: string; impact?: RoleImpact } | null>(null);
  const [deleteLoading, setDeleteLoading] = useState(false);
  const [editingKeyId, setEditingKeyId] = useState('');
  const [editingKeyName, setEditingKeyName] = useState('');
  const [error, setError] = useState('');
  const [pageError, setPageError] = useState('');
  const [showPasswordModal, setShowPasswordModal] = useState(false);
  const [passwordUserId, setPasswordUserId] = useState('');
  const [passwordUsername, setPasswordUsername] = useState('');
  const [newPassword, setNewPassword] = useState('');
  const [confirmPassword, setConfirmPassword] = useState('');
  const [passwordSuccess, setPasswordSuccess] = useState(false);
  const [isSubmitting, setIsSubmitting] = useState(false);

  useEffect(() => {
    loadData();
  }, []);

  const loadData = async () => {
    try {
      const [usersData, rolesData, keysData] = await Promise.all([
        api.getUsers(),
        api.getRoles(),
        api.getApiKeys().catch(() => [] as ApiKey[]),
      ]);
      setUsers(usersData);
      setRoles(rolesData);
      setApiKeys(keysData);
      setPageError('');
    } catch (e: any) {
      setPageError(e.message || 'Failed to load data');
    } finally {
      setLoading(false);
    }
  };

  const createUser = async () => {
    if (isSubmitting) return;
    setError('');
    setIsSubmitting(true);
    try {
      await api.createUser(newUser);
      setShowCreateModal(false);
      setNewUser({ username: '', password: '', email: '', role: 'owner' });
      loadData();
    } catch (e: any) {
      setError(e.message);
    } finally {
      setIsSubmitting(false);
    }
  };

  const updateRole = async (userId: string, role: string) => {
    try {
      await api.updateUser(userId, { role });
      loadData();
    } catch (e: any) {
      setPageError(e.message || 'Failed to update role');
    }
  };

  const openRoleCreate = () => {
    setEditingRole(null);
    setRoleForm({ name: '', description: '', default_policy: 'allow' });
    setError('');
    setShowRoleModal(true);
  };

  const openRoleEdit = (role: Role) => {
    setEditingRole(role);
    setRoleForm({ name: role.name, description: role.description || '', default_policy: role.default_policy || 'allow' });
    setError('');
    setShowRoleModal(true);
  };

  const saveRole = async () => {
    if (isSubmitting) return;
    setError('');
    setIsSubmitting(true);
    try {
      if (editingRole) {
        await api.updateRole(editingRole.role_id, {
          name: roleForm.name,
          description: roleForm.description || undefined,
          default_policy: roleForm.default_policy,
        });
      } else {
        await api.createRole({ name: roleForm.name, description: roleForm.description || undefined, default_policy: roleForm.default_policy });
      }
      setShowRoleModal(false);
      loadData();
    } catch (e: any) {
      setError(e.message);
    } finally {
      setIsSubmitting(false);
    }
  };

  const confirmDeleteRole = async (role: Role) => {
    try {
      const impact = await api.getRoleImpact(role.role_id);
      setDeleteConfirm({ type: 'role', id: role.role_id, name: role.name, impact });
    } catch (e: any) {
      setPageError(e.message || 'Failed to check role impact');
    }
  };

  const confirmDeleteUser = (user: User) => {
    setDeleteConfirm({ type: 'user', id: user.user_id, name: user.username });
  };

  const executeDelete = async () => {
    if (!deleteConfirm) return;
    setDeleteLoading(true);
    try {
      if (deleteConfirm.type === 'role') {
        await api.deleteRole(deleteConfirm.id);
      } else {
        await api.deleteUser(deleteConfirm.id);
      }
      setDeleteConfirm(null);
      loadData();
    } catch (e: any) {
      setPageError(e.message || `Failed to delete ${deleteConfirm.type}`);
      setDeleteConfirm(null);
    } finally {
      setDeleteLoading(false);
    }
  };

  const openKeyModal = (userId: string) => {
    setKeyModalUserId(userId);
    setKeyName('');
    setGeneratedKey('');
    setCopied(false);
    setShowKeyModal(true);
  };

  const generateApiKey = async () => {
    if (isSubmitting) return;
    setError('');
    setIsSubmitting(true);
    try {
      const result = await api.createApiKey({ name: keyName || 'default', user_id: keyModalUserId });
      setGeneratedKey(result.raw_key);
      setCopied(false);
      loadData();
    } catch (e: any) {
      setError(e.message);
    } finally {
      setIsSubmitting(false);
    }
  };

  const copyKey = () => {
    navigator.clipboard.writeText(generatedKey);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const revokeKey = async (keyId: string) => {
    try {
      await api.deleteApiKey(keyId);
      loadData();
    } catch (e: any) {
      setPageError(e.message || 'Failed to revoke key');
    }
  };

  const startRenameKey = (key: ApiKey) => {
    setEditingKeyId(key.key_id);
    setEditingKeyName(key.name);
  };

  const saveKeyName = async () => {
    if (!editingKeyId || !editingKeyName.trim()) return;
    try {
      await api.updateApiKey(editingKeyId, { name: editingKeyName.trim() });
      setEditingKeyId('');
      setEditingKeyName('');
      loadData();
    } catch (e: any) {
      setPageError(e.message || 'Failed to rename key');
    }
  };

  const cancelRenameKey = () => {
    setEditingKeyId('');
    setEditingKeyName('');
  };

  const openPasswordModal = (userId: string, username: string) => {
    setPasswordUserId(userId);
    setPasswordUsername(username);
    setNewPassword('');
    setConfirmPassword('');
    setError('');
    setPasswordSuccess(false);
    setShowPasswordModal(true);
  };

  const changePassword = async () => {
    if (isSubmitting) return;
    setError('');
    if (!newPassword.trim()) { setError('Password is required'); return; }
    if (newPassword.length < 4) { setError('Password must be at least 4 characters'); return; }
    if (newPassword !== confirmPassword) { setError('Passwords do not match'); return; }
    setIsSubmitting(true);
    try {
      const res = await api.updateUser(passwordUserId, { password: newPassword });
      // The server returns a fresh token only when you changed your OWN password
      // (the change revoked your current one). Store it so an admin changing their
      // own password stays logged in instead of a silent 401 on the next request.
      if (res.token) {
        localStorage.setItem('mcpgw_token', res.token);
      }
      setPasswordSuccess(true);
      setTimeout(() => {
        setShowPasswordModal(false);
        setPasswordSuccess(false);
      }, 1500);
    } catch (e: any) {
      setError(e.message || 'Failed to change password');
    } finally {
      setIsSubmitting(false);
    }
  };

  // A role is an *identity*, not a severity, so it gets emphasis rather than
  // hue — the ramp runs from the most privileged (loudest) down. The old
  // palette assigned colours by array index, which is how "owner" ended up
  // painted the same red as a failure purely for being first in the list. The
  // meaning on this page lives in the default-policy column, and that is the
  // only thing here allowed to be green or red.
  const ROLE_PALETTE = [
    'bg-neutral-wash text-ink border-line-strong font-semibold',
    'bg-neutral-wash text-ink-2 border-line',
    'bg-transparent text-ink-3 border-line',
    'bg-transparent text-ink-3 border-line',
    'bg-transparent text-ink-4 border-line',
    'bg-transparent text-ink-4 border-line',
  ];

  const getRoleColor = (roleName: string) => {
    const idx = roles.findIndex(r => r.name === roleName);
    return ROLE_PALETTE[idx >= 0 ? idx % ROLE_PALETTE.length : 0];
  };

  return (
    <div>
      <PageHeader
        title="Users"
        description="Who can reach the gateway, and which role decides what they may call. A role carries a default decision and the policies bound to it."
        actions={
          <>
            <Button icon={Plus} onClick={openRoleCreate}>
              Create role
            </Button>
            <Button variant="primary" icon={UserPlus} onClick={() => setShowCreateModal(true)}>
              Add user
            </Button>
          </>
        }
      />

      {pageError && (
        <Banner tone="deny" onDismiss={() => setPageError('')} className="mb-4">
          {pageError}
        </Banner>
      )}

      {/* Users table */}
      <Table>
          <thead>
            <tr>
              <Th>User</Th>
              <Th hide="sm">Role</Th>
              <Th hide="md">Last login</Th>
              <Th hide="lg">Created</Th>
              <Th align="right">Actions</Th>
            </tr>
          </thead>
          <tbody>
            {loading ? (
              <TableMessage colSpan={5}><Loading label="Loading users..." /></TableMessage>
            ) : users.map(user => (
              <tr key={user.user_id} className="border-b border-line-soft hover:bg-raised transition-colors">
                <td className="px-2.5 sm:px-4 py-3">
                  <div className="w-[min(150px,28vw)] sm:w-auto">
                    <p className="text-sm font-medium text-ink truncate">{user.username}</p>
                    <p className="text-xs text-ink-3 truncate">{user.email || 'No email'}</p>
                  </div>
                </td>
                <td className="px-2.5 sm:px-4 py-3 hidden sm:table-cell">
                  <div className="flex items-center gap-1.5">
                    {user.roles.map(role => (
                      <span key={role} className={clsx('inline-flex px-2 py-0.5 text-xs font-medium rounded-full border capitalize', getRoleColor(role))}>
                        {role}
                      </span>
                    ))}
                  </div>
                </td>
                <td className="px-2.5 sm:px-4 py-3 text-sm text-ink-2 hidden md:table-cell">
                  {user.last_login ? fmt.relative(user.last_login) : 'Never'}
                </td>
                <td className="px-2.5 sm:px-4 py-3 text-sm text-ink-2 hidden lg:table-cell">
                  {new Date(user.created_at).toLocaleDateString()}
                </td>
                <td className="px-2.5 sm:px-4 py-3">
                  <div className="flex items-center justify-end gap-1.5">
                    <select
                      value={user.roles[0] || 'owner'}
                      onChange={e => updateRole(user.user_id, e.target.value)}
                      aria-label={`Role for ${user.username}`}
                      className="text-xs px-2 py-1 bg-raised border border-line rounded-control text-ink-2 max-w-[70px] sm:max-w-none"
                    >
                      {roles.map(r => <option key={r.role_id} value={r.name}>{r.name}</option>)}
                    </select>
                    <button
                      onClick={() => openPasswordModal(user.user_id, user.username)}
                      className="text-xs px-1.5 sm:px-2 py-1 rounded-control transition-colors text-ink-2 hover:text-ink hover:bg-high"
                      title="Change password"
                    >
                      <Lock className="w-3.5 h-3.5" />
                    </button>
                    <button
                      onClick={() => openKeyModal(user.user_id)}
                      className="text-xs px-1.5 sm:px-2 py-1 rounded-control transition-colors text-beam hover:bg-beam-wash/10"
                      title="Generate API key"
                    >
                      <Key className="w-3.5 h-3.5" />
                    </button>
                    <button
                      onClick={() => confirmDeleteUser(user)}
                      className="p-0.5 sm:p-1 text-ink-4 hover:text-deny hover:bg-deny-wash/10 rounded-control transition-colors"
                      title="Delete user"
                    >
                      <Trash2 className="w-3.5 h-3.5" />
                    </button>
                  </div>
                </td>
              </tr>
            ))}
          </tbody>
      </Table>

      {/* Roles section */}
      <div className="bg-panel border border-line rounded-card overflow-hidden mt-6">
        <div className="px-2.5 sm:px-4 py-3 border-b border-line flex items-center justify-between">
          <h3 className="text-sm font-semibold text-ink flex items-center gap-2">
            <Shield className="w-4 h-4 text-beam" />
            Roles
          </h3>
        </div>
        <table className="w-full">
          <thead>
            <tr className="border-b border-line">
              <th className="text-left px-2.5 sm:px-4 py-3 text-xs font-medium text-ink-3 uppercase tracking-wider">Name</th>
              <th className="text-left px-2.5 sm:px-4 py-3 text-xs font-medium text-ink-3 uppercase tracking-wider">Description</th>
              <th className="text-left px-2.5 sm:px-4 py-3 text-xs font-medium text-ink-3 uppercase tracking-wider">Default</th>
              <th className="text-left px-2.5 sm:px-4 py-3 text-xs font-medium text-ink-3 uppercase tracking-wider">Users</th>
              <th className="text-left px-2.5 sm:px-4 py-3 text-xs font-medium text-ink-3 uppercase tracking-wider">Policies</th>
              <th className="text-left px-2.5 sm:px-4 py-3 text-xs font-medium text-ink-3 uppercase tracking-wider">Type</th>
              <th className="text-left px-2.5 sm:px-4 py-3 text-xs font-medium text-ink-3 uppercase tracking-wider">Actions</th>
            </tr>
          </thead>
          <tbody>
            {roles.map(role => (
              <tr key={role.role_id} className="border-b border-line-soft hover:bg-raised transition-colors">
                <td className="px-2.5 sm:px-4 py-3">
                  <span className={clsx('inline-flex px-2 py-0.5 text-xs font-medium rounded-full border capitalize', getRoleColor(role.name))}>
                    {role.name}
                  </span>
                </td>
                <td className="px-2.5 sm:px-4 py-3 text-sm text-ink-2">{role.description || '—'}</td>
                <td className="px-2.5 sm:px-4 py-3">
                  <span className={clsx(
                    'text-xs px-1.5 py-0.5 rounded-control font-medium',
                    role.default_policy === 'allow'
                      ? 'bg-beam-wash text-beam'
                      : 'bg-deny-wash text-deny'
                  )}>
                    {role.default_policy === 'allow' ? 'Allow all' : 'Deny all'}
                  </span>
                </td>
                <td className="px-2.5 sm:px-4 py-3 text-sm text-ink-2">{role.user_count}</td>
                <td className="px-2.5 sm:px-4 py-3 text-sm text-ink-2">{role.policies.length} active</td>
                <td className="px-2.5 sm:px-4 py-3">
                  <span className={clsx('text-xs px-1.5 py-0.5 rounded-control', role.is_system ? 'bg-high text-ink-2' : 'bg-beam-wash text-beam')}>
                    {role.is_system ? 'system' : 'custom'}
                  </span>
                </td>
                <td className="px-2.5 sm:px-4 py-3">
                  <div className="flex items-center gap-1">
                    <button
                      onClick={() => openRoleEdit(role)}
                      className="p-1.5 text-ink-3 hover:text-ink-2 hover:bg-high rounded-control transition-colors"
                      title="Edit role"
                    >
                      <Edit3 className="w-3.5 h-3.5" />
                    </button>
                    {!role.is_system && (
                      <button
                        onClick={() => confirmDeleteRole(role)}
                        className="p-1.5 text-ink-3 hover:text-deny hover:bg-deny-wash/10 rounded-control transition-colors"
                        title="Delete role"
                      >
                        <Trash2 className="w-3.5 h-3.5" />
                      </button>
                    )}
                  </div>
                </td>
              </tr>
            ))}
            {roles.length === 0 && (
              <tr><td colSpan={7} className="px-4 py-8 text-center text-ink-3 text-sm">No roles defined</td></tr>
            )}
          </tbody>
        </table>
      </div>

      {/* API Keys table */}
      {apiKeys.length > 0 && (
        <div className="bg-panel border border-line rounded-card overflow-hidden mt-6">
          <div className="px-2.5 sm:px-4 py-3 border-b border-line">
            <h3 className="text-sm font-semibold text-ink flex items-center gap-2">
              <Key className="w-4 h-4 text-beam" />
              API keys
            </h3>
          </div>
          <table className="w-full">
            <thead>
              <tr className="border-b border-line">
                <th className="text-left px-2.5 sm:px-4 py-3 text-xs font-medium text-ink-3 uppercase tracking-wider">Prefix</th>
                <th className="text-left px-2.5 sm:px-4 py-3 text-xs font-medium text-ink-3 uppercase tracking-wider">Name</th>
                <th className="text-left px-2.5 sm:px-4 py-3 text-xs font-medium text-ink-3 uppercase tracking-wider">User</th>
                <th className="text-left px-2.5 sm:px-4 py-3 text-xs font-medium text-ink-3 uppercase tracking-wider">Created</th>
                <th className="text-left px-2.5 sm:px-4 py-3 text-xs font-medium text-ink-3 uppercase tracking-wider">Last used</th>
                <th className="text-left px-2.5 sm:px-4 py-3 text-xs font-medium text-ink-3 uppercase tracking-wider">Actions</th>
              </tr>
            </thead>
            <tbody>
              {apiKeys.map(k => (
                <tr key={k.key_id} className="border-b border-line-soft hover:bg-raised transition-colors">
                  <td className="px-2.5 sm:px-4 py-3 text-sm text-ink-2 font-mono">{k.key_prefix}...</td>
                  <td className="px-2.5 sm:px-4 py-3 text-sm text-ink">
                    {editingKeyId === k.key_id ? (
                      <form onSubmit={e => { e.preventDefault(); saveKeyName(); }} className="flex items-center gap-1.5">
                        <input
                          type="text"
                          value={editingKeyName}
                          onChange={e => setEditingKeyName(e.target.value)}
                          className="px-2 py-0.5 bg-inset border border-beam-edge rounded-control text-sm text-ink focus:outline-none w-36"
                          autoFocus
                          onKeyDown={e => { if (e.key === 'Escape') cancelRenameKey(); }}
                        />
                        <button type="submit" className="p-0.5 text-beam hover:text-beam/80" title="Save">
                          <Check className="w-3.5 h-3.5" />
                        </button>
                        <button type="button" onClick={cancelRenameKey} className="p-0.5 text-ink-3 hover:text-ink-2" title="Cancel">
                          <X className="w-3.5 h-3.5" />
                        </button>
                      </form>
                    ) : (
                      k.name
                    )}
                  </td>
                  <td className="px-2.5 sm:px-4 py-3 text-sm text-ink-2">{k.username}</td>
                  <td className="px-2.5 sm:px-4 py-3 text-sm text-ink-2">{new Date(k.created_at).toLocaleDateString()}</td>
                  <td className="px-2.5 sm:px-4 py-3 text-sm text-ink-2">{k.last_used ? new Date(k.last_used).toLocaleString() : 'Never'}</td>
                  <td className="px-2.5 sm:px-4 py-3">
                    <div className="flex items-center gap-1">
                      <button
                        onClick={() => startRenameKey(k)}
                        className="p-1.5 text-ink-3 hover:text-ink-2 hover:bg-high rounded-control transition-colors"
                        title="Rename key"
                      >
                        <Edit3 className="w-3.5 h-3.5" />
                      </button>
                      <button
                        onClick={() => revokeKey(k.key_id)}
                        className="text-xs px-2 py-1 rounded-control text-deny hover:bg-deny-wash/10 transition-colors"
                      >
                        Revoke
                      </button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {/* Generate API Key modal */}
      {showKeyModal && (
        <div className="fixed inset-0 bg-scrim flex items-center justify-center z-50">
          <div className="bg-panel border border-line rounded-panel p-6 w-full max-w-md">
            <div className="flex items-center justify-between mb-5">
              <h3 className="text-md font-semibold text-ink">Generate API key</h3>
              <button onClick={() => setShowKeyModal(false)} className="text-ink-3 hover:text-ink-2">
                <X className="w-5 h-5" />
              </button>
            </div>

            {!generatedKey ? (
              <div className="space-y-4">
                <div>
                  <label className="block text-xs font-medium text-ink-2 mb-1.5 uppercase tracking-wider">Key name</label>
                  <input
                    type="text"
                    value={keyName}
                    onChange={e => setKeyName(e.target.value)}
                    className="w-full px-3 py-2 bg-inset border border-line rounded-row text-sm text-ink focus:outline-none focus:border-beam-edge"
                    placeholder="e.g. claude-desktop"
                  />
                </div>
                {error && (
                  <div className="px-3 py-2 bg-deny-wash border border-deny-edge rounded-row">
                    <p className="text-xs text-deny">{error}</p>
                  </div>
                )}
                <div className="flex gap-3 pt-2">
                  <button
                    onClick={() => setShowKeyModal(false)}
                    className="flex-1 py-2 bg-raised border border-line text-ink-2 text-sm rounded-row hover:bg-high transition-colors"
                  >
                    Cancel
                  </button>
                  <button
                    onClick={generateApiKey}
                    disabled={isSubmitting}
                    className="flex-1 py-2 bg-solid hover:bg-solid-hover text-on-solid text-sm font-medium rounded-row transition-colors disabled:opacity-50"
                  >
                    {isSubmitting ? 'Generating...' : 'Generate'}
                  </button>
                </div>
              </div>
            ) : (
              <div className="space-y-4">
                <div className="flex items-start gap-2 px-3 py-2 bg-yellow-500/10 border border-yellow-500/20 rounded-row">
                  <AlertTriangle className="w-4 h-4 text-yellow-400 mt-0.5 flex-shrink-0" />
                  <p className="text-xs text-yellow-300">Copy this key now. It will not be shown again.</p>
                </div>
                <div className="flex items-center gap-2">
                  <code className="flex-1 px-3 py-2 bg-inset border border-line rounded-row text-xs text-beam font-mono break-all select-all">
                    {generatedKey}
                  </code>
                  <button
                    onClick={copyKey}
                    className="p-2 rounded-row bg-raised border border-line hover:bg-high transition-colors"
                    title="Copy to clipboard"
                  >
                    {copied ? <Check className="w-4 h-4 text-beam" /> : <Copy className="w-4 h-4 text-ink-2" />}
                  </button>
                </div>
                <button
                  onClick={() => setShowKeyModal(false)}
                  className="w-full py-2 bg-raised border border-line text-ink-2 text-sm rounded-row hover:bg-high transition-colors"
                >
                  Done
                </button>
              </div>
            )}
          </div>
        </div>
      )}

      {/* Create/Edit role modal */}
      {showRoleModal && (
        <div className="fixed inset-0 bg-scrim flex items-center justify-center z-50">
          <div className="bg-panel border border-line rounded-panel p-6 w-full max-w-md">
            <div className="flex items-center justify-between mb-5">
              <h3 className="text-md font-semibold text-ink">{editingRole ? 'Edit role' : 'Create role'}</h3>
              <button onClick={() => setShowRoleModal(false)} className="text-ink-3 hover:text-ink-2">
                <X className="w-5 h-5" />
              </button>
            </div>
            <div className="space-y-4">
              <div>
                <label className="block text-xs font-medium text-ink-2 mb-1.5 uppercase tracking-wider">Role name</label>
                <input
                  type="text"
                  value={roleForm.name}
                  onChange={e => setRoleForm({ ...roleForm, name: e.target.value })}
                  className="w-full px-3 py-2 bg-inset border border-line rounded-row text-sm text-ink focus:outline-none focus:border-beam-edge"
                  placeholder="e.g. analyst"
                />
              </div>
              <div>
                <label className="block text-xs font-medium text-ink-2 mb-1.5 uppercase tracking-wider">Description (optional)</label>
                <input
                  type="text"
                  value={roleForm.description}
                  onChange={e => setRoleForm({ ...roleForm, description: e.target.value })}
                  className="w-full px-3 py-2 bg-inset border border-line rounded-row text-sm text-ink focus:outline-none focus:border-beam-edge"
                  placeholder="e.g. Read-only analyst"
                />
              </div>
              <div>
                <label className="block text-xs font-medium text-ink-2 mb-1.5 uppercase tracking-wider">Default policy</label>
                <p className="text-xs text-ink-3 mb-2">When no policy rule matches a tool, this decides what happens.</p>
                <div className="flex gap-2">
                  <button
                    type="button"
                    onClick={() => setRoleForm({ ...roleForm, default_policy: 'allow' })}
                    className={clsx(
                      'flex-1 py-2.5 px-3 rounded-row text-sm font-medium border transition-colors',
                      roleForm.default_policy === 'allow'
                        ? 'bg-beam-wash border-beam-edge text-beam'
                        : 'bg-inset border-line text-ink-3 hover:border-line-strong'
                    )}
                  >
                    Allow all by default
                  </button>
                  <button
                    type="button"
                    onClick={() => setRoleForm({ ...roleForm, default_policy: 'deny' })}
                    className={clsx(
                      'flex-1 py-2.5 px-3 rounded-row text-sm font-medium border transition-colors',
                      roleForm.default_policy === 'deny'
                        ? 'bg-deny-wash border-deny-edge text-deny'
                        : 'bg-inset border-line text-ink-3 hover:border-line-strong'
                    )}
                  >
                    Deny all by default
                  </button>
                </div>
                <p className="text-xs text-ink-4 mt-2">
                  {roleForm.default_policy === 'allow'
                    ? 'All tools are allowed unless a deny policy explicitly blocks them.'
                    : 'All tools are blocked unless an allow policy explicitly grants access.'}
                </p>
              </div>
              {error && (
                <div className="px-3 py-2 bg-deny-wash border border-deny-edge rounded-row">
                  <p className="text-xs text-deny">{error}</p>
                </div>
              )}
              <div className="flex gap-3 pt-2">
                <button
                  onClick={() => setShowRoleModal(false)}
                  className="flex-1 py-2 bg-raised border border-line text-ink-2 text-sm rounded-row hover:bg-high transition-colors"
                >
                  Cancel
                </button>
                <button
                  onClick={saveRole}
                  disabled={isSubmitting}
                  className="flex-1 py-2 bg-solid hover:bg-solid-hover text-on-solid text-sm font-medium rounded-row transition-colors disabled:opacity-50"
                >
                  {isSubmitting ? 'Saving...' : editingRole ? 'Save changes' : 'Create role'}
                </button>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* Create user modal */}
      {showCreateModal && (
        <div className="fixed inset-0 bg-scrim flex items-center justify-center z-50">
          <div className="bg-panel border border-line rounded-panel p-6 w-full max-w-md">
            <div className="flex items-center justify-between mb-5">
              <h3 className="text-md font-semibold text-ink">Create user</h3>
              <button onClick={() => setShowCreateModal(false)} className="text-ink-3 hover:text-ink-2">
                <X className="w-5 h-5" />
              </button>
            </div>

            <div className="space-y-4">
              <div>
                <label className="block text-xs font-medium text-ink-2 mb-1.5 uppercase tracking-wider">Username</label>
                <input
                  type="text"
                  value={newUser.username}
                  onChange={e => setNewUser({ ...newUser, username: e.target.value })}
                  className="w-full px-3 py-2 bg-inset border border-line rounded-row text-sm text-ink focus:outline-none focus:border-beam-edge"
                />
              </div>
              <div>
                <label className="block text-xs font-medium text-ink-2 mb-1.5 uppercase tracking-wider">Password</label>
                <input
                  type="password"
                  value={newUser.password}
                  onChange={e => setNewUser({ ...newUser, password: e.target.value })}
                  className="w-full px-3 py-2 bg-inset border border-line rounded-row text-sm text-ink focus:outline-none focus:border-beam-edge"
                  placeholder="••••••••"
                />
              </div>
              <div>
                <label className="block text-xs font-medium text-ink-2 mb-1.5 uppercase tracking-wider">Email (optional)</label>
                <input
                  type="email"
                  value={newUser.email}
                  onChange={e => setNewUser({ ...newUser, email: e.target.value })}
                  className="w-full px-3 py-2 bg-inset border border-line rounded-row text-sm text-ink focus:outline-none focus:border-beam-edge"
                  placeholder="name@example.com"
                />
              </div>
              <div>
                <label className="block text-xs font-medium text-ink-2 mb-1.5 uppercase tracking-wider">Role</label>
                <select
                  value={newUser.role}
                  onChange={e => setNewUser({ ...newUser, role: e.target.value })}
                  className="w-full px-3 py-2 bg-inset border border-line rounded-row text-sm text-ink-2 focus:outline-none focus:border-beam-edge"
                >
                  {roles.map(r => <option key={r.role_id} value={r.name}>{r.name}</option>)}
                </select>
              </div>

              {error && (
                <div className="px-3 py-2 bg-deny-wash border border-deny-edge rounded-row">
                  <p className="text-xs text-deny">{error}</p>
                </div>
              )}

              <div className="flex gap-3 pt-2">
                <button
                  onClick={() => setShowCreateModal(false)}
                  className="flex-1 py-2 bg-raised border border-line text-ink-2 text-sm rounded-row hover:bg-high transition-colors"
                >
                  Cancel
                </button>
                <button
                  onClick={createUser}
                  disabled={isSubmitting}
                  className="flex-1 py-2 bg-solid hover:bg-solid-hover text-on-solid text-sm font-medium rounded-row transition-colors disabled:opacity-50"
                >
                  {isSubmitting ? 'Creating...' : 'Create user'}
                </button>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* Change password modal */}
      {showPasswordModal && (
        <div className="fixed inset-0 bg-scrim flex items-center justify-center z-50">
          <div className="bg-panel border border-line rounded-panel p-6 w-full max-w-md">
            <div className="flex items-center justify-between mb-5">
              <div>
                <h3 className="text-md font-semibold text-ink">Change password</h3>
                <p className="text-xs text-ink-3 mt-0.5">for {passwordUsername}</p>
              </div>
              <button onClick={() => setShowPasswordModal(false)} className="text-ink-3 hover:text-ink-2">
                <X className="w-5 h-5" />
              </button>
            </div>

            {passwordSuccess ? (
              <div className="flex items-center gap-3 py-8 justify-center">
                <Check className="w-6 h-6 text-beam" />
                <span className="text-sm text-beam font-medium">Password updated successfully</span>
              </div>
            ) : (
              <div className="space-y-4">
                <div>
                  <label className="block text-xs font-medium text-ink-2 mb-1.5 uppercase tracking-wider">New password</label>
                  <input
                    type="password"
                    value={newPassword}
                    onChange={e => setNewPassword(e.target.value)}
                    className="w-full px-3 py-2 bg-inset border border-line rounded-row text-sm text-ink focus:outline-none focus:border-beam-edge"
                    placeholder="••••••••"
                  />
                </div>
                <div>
                  <label className="block text-xs font-medium text-ink-2 mb-1.5 uppercase tracking-wider">Confirm password</label>
                  <input
                    type="password"
                    value={confirmPassword}
                    onChange={e => setConfirmPassword(e.target.value)}
                    className="w-full px-3 py-2 bg-inset border border-line rounded-row text-sm text-ink focus:outline-none focus:border-beam-edge"
                    placeholder="••••••••"
                  />
                </div>

                {error && (
                  <div className="px-3 py-2 bg-deny-wash border border-deny-edge rounded-row">
                    <p className="text-xs text-deny">{error}</p>
                  </div>
                )}

                <div className="flex gap-3 pt-2">
                  <button
                    onClick={() => setShowPasswordModal(false)}
                    className="flex-1 py-2 bg-raised border border-line text-ink-2 text-sm rounded-row hover:bg-high transition-colors"
                  >
                    Cancel
                  </button>
                  <button
                    onClick={changePassword}
                    disabled={isSubmitting}
                    className="flex-1 py-2 bg-solid hover:bg-solid-hover text-on-solid text-sm font-medium rounded-row transition-colors disabled:opacity-50"
                  >
                    {isSubmitting ? 'Saving...' : 'Change password'}
                  </button>
                </div>
              </div>
            )}
          </div>
        </div>
      )}

      {/* Delete confirmation dialog */}
      {deleteConfirm && (
        <div className="fixed inset-0 bg-scrim flex items-center justify-center z-50">
          <div className="bg-panel border border-line rounded-panel p-6 w-full max-w-md">
            <div className="flex items-center gap-3 mb-4">
              <div className="w-10 h-10 rounded-card bg-deny-wash flex items-center justify-center">
                <AlertTriangle className="w-5 h-5 text-deny" />
              </div>
              <div>
                <h3 className="text-md font-semibold text-ink">
                  Delete this {deleteConfirm.type}?
                </h3>
                <p className="text-sm text-ink-2">
                  This cannot be undone.
                </p>
              </div>
            </div>

            <div className="space-y-3 mb-5">
              <p className="text-sm text-ink-2">
                <span className="font-semibold text-ink">{deleteConfirm.name}</span> will be deleted.
              </p>

              {deleteConfirm.type === 'role' && deleteConfirm.impact && (
                <div className="bg-inset border border-line rounded-row p-3 space-y-2">
                  <p className="text-xs font-medium text-ink-2 uppercase tracking-wider">Cascading impact</p>

                  <div className="flex items-center justify-between text-sm">
                    <span className="text-ink-2">User assignments removed</span>
                    <span className="text-ink font-medium">{deleteConfirm.impact.affected_user_count}</span>
                  </div>

                  <div className="flex items-center justify-between text-sm">
                    <span className="text-ink-2">Policy bindings removed</span>
                    <span className="text-ink font-medium">{deleteConfirm.impact.policy_binding_count}</span>
                  </div>

                  {deleteConfirm.impact.orphaned_users.length > 0 && (
                    <div className="mt-2 px-3 py-2 bg-yellow-500/10 border border-yellow-500/20 rounded-row">
                      <div className="flex items-start gap-2">
                        <AlertTriangle className="w-4 h-4 text-yellow-400 mt-0.5 shrink-0" />
                        <div>
                          <p className="text-xs text-yellow-300 font-medium">Users left with no roles</p>
                          <p className="text-xs text-yellow-300/70 mt-0.5">
                            {deleteConfirm.impact.orphaned_users.join(', ')} will have no role assigned and may lose access.
                          </p>
                        </div>
                      </div>
                    </div>
                  )}

                  {deleteConfirm.impact.affected_users.length > 0 && (
                    <div className="text-xs text-ink-3 mt-1">
                      Affected users: {deleteConfirm.impact.affected_users.join(', ')}
                    </div>
                  )}
                </div>
              )}

              {deleteConfirm.type === 'user' && (
                <div className="bg-inset border border-line rounded-row p-3 space-y-1">
                  <p className="text-xs font-medium text-ink-2 uppercase tracking-wider">What will be removed</p>
                  <ul className="text-sm text-ink-2 space-y-0.5">
                    <li>All role assignments for this user</li>
                    <li>All API keys belonging to this user</li>
                    <li>Policies created by this user remain, but lose their creator reference</li>
                  </ul>
                </div>
              )}
            </div>

            <div className="flex gap-3">
              <button
                onClick={() => setDeleteConfirm(null)}
                disabled={deleteLoading}
                className="flex-1 py-2 bg-raised border border-line text-ink-2 text-sm rounded-row hover:bg-high transition-colors"
              >
                Cancel
              </button>
              <button
                onClick={executeDelete}
                disabled={deleteLoading}
                className="flex-1 py-2 bg-deny hover:opacity-90 text-on-solid text-sm font-medium rounded-row transition-colors disabled:opacity-50"
              >
                {deleteLoading ? 'Deleting...' : `Delete ${deleteConfirm.type}`}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
