import { useState, useEffect } from 'react';
import { api, User, Role, ApiKey, RoleImpact } from '@/lib/api';
import { UserPlus, X, Key, Copy, Check, AlertTriangle, Plus, Edit3, Trash2, Shield, Lock } from 'lucide-react';
import clsx from 'clsx';
import { useAuth } from '@/hooks/useAuth';

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
      await api.updateUser(passwordUserId, { password: newPassword });
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

  const ROLE_PALETTE = [
    'bg-red-500/10 text-red-400 border-red-500/20',
    'bg-blue-500/10 text-blue-400 border-blue-500/20',
    'bg-emerald-500/10 text-emerald-400 border-emerald-500/20',
    'bg-purple-500/10 text-purple-400 border-purple-500/20',
    'bg-amber-500/10 text-amber-400 border-amber-500/20',
    'bg-cyan-500/10 text-cyan-400 border-cyan-500/20',
  ];

  const getRoleColor = (roleName: string) => {
    const idx = roles.findIndex(r => r.name === roleName);
    return ROLE_PALETTE[idx >= 0 ? idx % ROLE_PALETTE.length : 0];
  };

  return (
    <div>
      <div className="flex items-center justify-between mb-6">
        <div>
          <h2 className="text-lg font-semibold text-white">User Management</h2>
          <p className="text-sm text-gray-500 mt-1">Manage users and role assignments</p>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={openRoleCreate}
            className="flex items-center gap-2 px-3 py-2 bg-surface border border-border rounded-lg text-sm text-gray-300 hover:border-border-hover transition-colors"
          >
            <Plus className="w-4 h-4" />
            Create Role
          </button>
          <button
            onClick={() => setShowCreateModal(true)}
            className="flex items-center gap-2 px-4 py-2 bg-accent hover:bg-accent-hover text-white text-sm font-medium rounded-lg transition-colors"
          >
            <UserPlus className="w-4 h-4" />
            Add User
          </button>
        </div>
      </div>

      {/* Page-level error */}
      {pageError && (
        <div className="mb-4 px-4 py-3 bg-danger/10 border border-danger/20 rounded-lg flex items-center justify-between">
          <p className="text-sm text-danger">{pageError}</p>
          <button onClick={() => setPageError('')} className="text-danger/60 hover:text-danger text-xs ml-4">dismiss</button>
        </div>
      )}

      {/* Users table */}
      <div className="bg-surface border border-border rounded-xl overflow-hidden">
        <table className="w-full">
          <thead>
            <tr className="border-b border-border">
              <th className="text-left px-4 py-3 text-xs font-medium text-gray-500 uppercase tracking-wider">User</th>
              <th className="text-left px-4 py-3 text-xs font-medium text-gray-500 uppercase tracking-wider">Role</th>
              <th className="text-left px-4 py-3 text-xs font-medium text-gray-500 uppercase tracking-wider">Last Login</th>
              <th className="text-left px-4 py-3 text-xs font-medium text-gray-500 uppercase tracking-wider">Created</th>
              <th className="text-left px-4 py-3 text-xs font-medium text-gray-500 uppercase tracking-wider">Actions</th>
            </tr>
          </thead>
          <tbody>
            {loading ? (
              <tr><td colSpan={5} className="px-4 py-12 text-center text-gray-500 text-sm">Loading...</td></tr>
            ) : users.map(user => (
              <tr key={user.user_id} className="border-b border-border/50 hover:bg-surface-hover transition-colors">
                <td className="px-4 py-3">
                  <div>
                    <p className="text-sm font-medium text-white">{user.username}</p>
                    <p className="text-xs text-gray-500">{user.email || 'No email'}</p>
                  </div>
                </td>
                <td className="px-4 py-3">
                  <div className="flex items-center gap-1.5">
                    {user.roles.map(role => (
                      <span key={role} className={clsx('inline-flex px-2 py-0.5 text-xs font-medium rounded-full border capitalize', getRoleColor(role))}>
                        {role}
                      </span>
                    ))}
                  </div>
                </td>
                <td className="px-4 py-3 text-sm text-gray-400">
                  {user.last_login ? new Date(user.last_login).toLocaleDateString() : 'Never'}
                </td>
                <td className="px-4 py-3 text-sm text-gray-400">
                  {new Date(user.created_at).toLocaleDateString()}
                </td>
                <td className="px-4 py-3">
                  <div className="flex items-center gap-2">
                    <select
                      value={user.roles[0] || 'owner'}
                      onChange={e => updateRole(user.user_id, e.target.value)}
                      className="text-xs px-2 py-1 bg-surface-hover border border-border rounded text-gray-300"
                    >
                      {roles.map(r => <option key={r.role_id} value={r.name}>{r.name}</option>)}
                    </select>
                    <button
                      onClick={() => openPasswordModal(user.user_id, user.username)}
                      className="text-xs px-2 py-1 rounded transition-colors text-gray-400 hover:text-white hover:bg-surface-active"
                      title="Change password"
                    >
                      <Lock className="w-3.5 h-3.5" />
                    </button>
                    <button
                      onClick={() => openKeyModal(user.user_id)}
                      className="text-xs px-2 py-1 rounded transition-colors text-accent hover:bg-accent/10"
                      title="Generate API Key"
                    >
                      <Key className="w-3.5 h-3.5" />
                    </button>
                    <button
                      onClick={() => confirmDeleteUser(user)}
                      className="p-1 text-gray-600 hover:text-danger hover:bg-danger/10 rounded transition-colors"
                      title="Delete user"
                    >
                      <Trash2 className="w-3.5 h-3.5" />
                    </button>
                  </div>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      {/* Roles section */}
      <div className="bg-surface border border-border rounded-xl overflow-hidden mt-6">
        <div className="px-4 py-3 border-b border-border flex items-center justify-between">
          <h3 className="text-sm font-semibold text-white flex items-center gap-2">
            <Shield className="w-4 h-4 text-accent" />
            Roles
          </h3>
        </div>
        <table className="w-full">
          <thead>
            <tr className="border-b border-border">
              <th className="text-left px-4 py-3 text-xs font-medium text-gray-500 uppercase tracking-wider">Name</th>
              <th className="text-left px-4 py-3 text-xs font-medium text-gray-500 uppercase tracking-wider">Description</th>
              <th className="text-left px-4 py-3 text-xs font-medium text-gray-500 uppercase tracking-wider">Default</th>
              <th className="text-left px-4 py-3 text-xs font-medium text-gray-500 uppercase tracking-wider">Users</th>
              <th className="text-left px-4 py-3 text-xs font-medium text-gray-500 uppercase tracking-wider">Policies</th>
              <th className="text-left px-4 py-3 text-xs font-medium text-gray-500 uppercase tracking-wider">Type</th>
              <th className="text-left px-4 py-3 text-xs font-medium text-gray-500 uppercase tracking-wider">Actions</th>
            </tr>
          </thead>
          <tbody>
            {roles.map(role => (
              <tr key={role.role_id} className="border-b border-border/50 hover:bg-surface-hover transition-colors">
                <td className="px-4 py-3">
                  <span className={clsx('inline-flex px-2 py-0.5 text-xs font-medium rounded-full border capitalize', getRoleColor(role.name))}>
                    {role.name}
                  </span>
                </td>
                <td className="px-4 py-3 text-sm text-gray-400">{role.description || '-'}</td>
                <td className="px-4 py-3">
                  <span className={clsx(
                    'text-xs px-1.5 py-0.5 rounded font-medium',
                    role.default_policy === 'allow'
                      ? 'bg-emerald-500/10 text-emerald-400'
                      : 'bg-red-500/10 text-red-400'
                  )}>
                    {role.default_policy === 'allow' ? 'Allow all' : 'Deny all'}
                  </span>
                </td>
                <td className="px-4 py-3 text-sm text-gray-300">{role.user_count}</td>
                <td className="px-4 py-3 text-sm text-gray-400">{role.policies.length} active</td>
                <td className="px-4 py-3">
                  <span className={clsx('text-xs px-1.5 py-0.5 rounded', role.is_system ? 'bg-surface-active text-gray-400' : 'bg-accent/10 text-accent')}>
                    {role.is_system ? 'system' : 'custom'}
                  </span>
                </td>
                <td className="px-4 py-3">
                  <div className="flex items-center gap-1">
                    <button
                      onClick={() => openRoleEdit(role)}
                      className="p-1.5 text-gray-500 hover:text-gray-300 hover:bg-surface-active rounded-md transition-colors"
                      title="Edit role"
                    >
                      <Edit3 className="w-3.5 h-3.5" />
                    </button>
                    {!role.is_system && (
                      <button
                        onClick={() => confirmDeleteRole(role)}
                        className="p-1.5 text-gray-500 hover:text-danger hover:bg-danger/10 rounded-md transition-colors"
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
              <tr><td colSpan={7} className="px-4 py-8 text-center text-gray-500 text-sm">No roles defined</td></tr>
            )}
          </tbody>
        </table>
      </div>

      {/* API Keys table */}
      {apiKeys.length > 0 && (
        <div className="bg-surface border border-border rounded-xl overflow-hidden mt-6">
          <div className="px-4 py-3 border-b border-border">
            <h3 className="text-sm font-semibold text-white flex items-center gap-2">
              <Key className="w-4 h-4 text-accent" />
              API Keys
            </h3>
          </div>
          <table className="w-full">
            <thead>
              <tr className="border-b border-border">
                <th className="text-left px-4 py-3 text-xs font-medium text-gray-500 uppercase tracking-wider">Prefix</th>
                <th className="text-left px-4 py-3 text-xs font-medium text-gray-500 uppercase tracking-wider">Name</th>
                <th className="text-left px-4 py-3 text-xs font-medium text-gray-500 uppercase tracking-wider">User</th>
                <th className="text-left px-4 py-3 text-xs font-medium text-gray-500 uppercase tracking-wider">Created</th>
                <th className="text-left px-4 py-3 text-xs font-medium text-gray-500 uppercase tracking-wider">Last Used</th>
                <th className="text-left px-4 py-3 text-xs font-medium text-gray-500 uppercase tracking-wider">Actions</th>
              </tr>
            </thead>
            <tbody>
              {apiKeys.map(k => (
                <tr key={k.key_id} className="border-b border-border/50 hover:bg-surface-hover transition-colors">
                  <td className="px-4 py-3 text-sm text-gray-300 font-mono">{k.key_prefix}...</td>
                  <td className="px-4 py-3 text-sm text-white">
                    {editingKeyId === k.key_id ? (
                      <form onSubmit={e => { e.preventDefault(); saveKeyName(); }} className="flex items-center gap-1.5">
                        <input
                          type="text"
                          value={editingKeyName}
                          onChange={e => setEditingKeyName(e.target.value)}
                          className="px-2 py-0.5 bg-[#0a0a0f] border border-accent/50 rounded text-sm text-white focus:outline-none w-36"
                          autoFocus
                          onKeyDown={e => { if (e.key === 'Escape') cancelRenameKey(); }}
                        />
                        <button type="submit" className="p-0.5 text-success hover:text-success/80" title="Save">
                          <Check className="w-3.5 h-3.5" />
                        </button>
                        <button type="button" onClick={cancelRenameKey} className="p-0.5 text-gray-500 hover:text-gray-300" title="Cancel">
                          <X className="w-3.5 h-3.5" />
                        </button>
                      </form>
                    ) : (
                      k.name
                    )}
                  </td>
                  <td className="px-4 py-3 text-sm text-gray-400">{k.username}</td>
                  <td className="px-4 py-3 text-sm text-gray-400">{new Date(k.created_at).toLocaleDateString()}</td>
                  <td className="px-4 py-3 text-sm text-gray-400">{k.last_used ? new Date(k.last_used).toLocaleString() : 'Never'}</td>
                  <td className="px-4 py-3">
                    <div className="flex items-center gap-1">
                      <button
                        onClick={() => startRenameKey(k)}
                        className="p-1.5 text-gray-500 hover:text-gray-300 hover:bg-surface-active rounded-md transition-colors"
                        title="Rename key"
                      >
                        <Edit3 className="w-3.5 h-3.5" />
                      </button>
                      <button
                        onClick={() => revokeKey(k.key_id)}
                        className="text-xs px-2 py-1 rounded text-danger hover:bg-danger/10 transition-colors"
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
        <div className="fixed inset-0 bg-black/60 flex items-center justify-center z-50">
          <div className="bg-surface border border-border rounded-2xl p-6 w-full max-w-md">
            <div className="flex items-center justify-between mb-5">
              <h3 className="text-base font-semibold text-white">Generate API Key</h3>
              <button onClick={() => setShowKeyModal(false)} className="text-gray-500 hover:text-gray-300">
                <X className="w-5 h-5" />
              </button>
            </div>

            {!generatedKey ? (
              <div className="space-y-4">
                <div>
                  <label className="block text-xs font-medium text-gray-400 mb-1.5 uppercase tracking-wider">Key Name</label>
                  <input
                    type="text"
                    value={keyName}
                    onChange={e => setKeyName(e.target.value)}
                    className="w-full px-3 py-2 bg-[#0a0a0f] border border-border rounded-lg text-sm text-white focus:outline-none focus:border-accent/50"
                    placeholder="e.g. claude-desktop"
                  />
                </div>
                {error && (
                  <div className="px-3 py-2 bg-danger/10 border border-danger/20 rounded-lg">
                    <p className="text-xs text-danger">{error}</p>
                  </div>
                )}
                <div className="flex gap-3 pt-2">
                  <button
                    onClick={() => setShowKeyModal(false)}
                    className="flex-1 py-2 bg-surface-hover border border-border text-gray-300 text-sm rounded-lg hover:bg-surface-active transition-colors"
                  >
                    Cancel
                  </button>
                  <button
                    onClick={generateApiKey}
                    disabled={isSubmitting}
                    className="flex-1 py-2 bg-accent hover:bg-accent-hover text-white text-sm font-medium rounded-lg transition-colors disabled:opacity-50"
                  >
                    {isSubmitting ? 'Generating…' : 'Generate'}
                  </button>
                </div>
              </div>
            ) : (
              <div className="space-y-4">
                <div className="flex items-start gap-2 px-3 py-2 bg-yellow-500/10 border border-yellow-500/20 rounded-lg">
                  <AlertTriangle className="w-4 h-4 text-yellow-400 mt-0.5 flex-shrink-0" />
                  <p className="text-xs text-yellow-300">Copy this key now. It will not be shown again.</p>
                </div>
                <div className="flex items-center gap-2">
                  <code className="flex-1 px-3 py-2 bg-[#0a0a0f] border border-border rounded-lg text-xs text-emerald-400 font-mono break-all select-all">
                    {generatedKey}
                  </code>
                  <button
                    onClick={copyKey}
                    className="p-2 rounded-lg bg-surface-hover border border-border hover:bg-surface-active transition-colors"
                    title="Copy to clipboard"
                  >
                    {copied ? <Check className="w-4 h-4 text-success" /> : <Copy className="w-4 h-4 text-gray-400" />}
                  </button>
                </div>
                <button
                  onClick={() => setShowKeyModal(false)}
                  className="w-full py-2 bg-surface-hover border border-border text-gray-300 text-sm rounded-lg hover:bg-surface-active transition-colors"
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
        <div className="fixed inset-0 bg-black/60 flex items-center justify-center z-50">
          <div className="bg-surface border border-border rounded-2xl p-6 w-full max-w-md">
            <div className="flex items-center justify-between mb-5">
              <h3 className="text-base font-semibold text-white">{editingRole ? 'Edit Role' : 'Create Role'}</h3>
              <button onClick={() => setShowRoleModal(false)} className="text-gray-500 hover:text-gray-300">
                <X className="w-5 h-5" />
              </button>
            </div>
            <div className="space-y-4">
              <div>
                <label className="block text-xs font-medium text-gray-400 mb-1.5 uppercase tracking-wider">Role Name</label>
                <input
                  type="text"
                  value={roleForm.name}
                  onChange={e => setRoleForm({ ...roleForm, name: e.target.value })}
                  className="w-full px-3 py-2 bg-[#0a0a0f] border border-border rounded-lg text-sm text-white focus:outline-none focus:border-accent/50"
                  placeholder="e.g. analyst"
                />
              </div>
              <div>
                <label className="block text-xs font-medium text-gray-400 mb-1.5 uppercase tracking-wider">Description</label>
                <input
                  type="text"
                  value={roleForm.description}
                  onChange={e => setRoleForm({ ...roleForm, description: e.target.value })}
                  className="w-full px-3 py-2 bg-[#0a0a0f] border border-border rounded-lg text-sm text-white focus:outline-none focus:border-accent/50"
                  placeholder="Optional description"
                />
              </div>
              <div>
                <label className="block text-xs font-medium text-gray-400 mb-1.5 uppercase tracking-wider">Default Policy</label>
                <p className="text-xs text-gray-500 mb-2">When no policy rule matches a tool, this decides what happens.</p>
                <div className="flex gap-2">
                  <button
                    type="button"
                    onClick={() => setRoleForm({ ...roleForm, default_policy: 'allow' })}
                    className={clsx(
                      'flex-1 py-2.5 px-3 rounded-lg text-sm font-medium border transition-colors',
                      roleForm.default_policy === 'allow'
                        ? 'bg-emerald-500/15 border-emerald-500/40 text-emerald-400'
                        : 'bg-[#0a0a0f] border-border text-gray-500 hover:border-border-hover'
                    )}
                  >
                    Allow all by default
                  </button>
                  <button
                    type="button"
                    onClick={() => setRoleForm({ ...roleForm, default_policy: 'deny' })}
                    className={clsx(
                      'flex-1 py-2.5 px-3 rounded-lg text-sm font-medium border transition-colors',
                      roleForm.default_policy === 'deny'
                        ? 'bg-red-500/15 border-red-500/40 text-red-400'
                        : 'bg-[#0a0a0f] border-border text-gray-500 hover:border-border-hover'
                    )}
                  >
                    Deny all by default
                  </button>
                </div>
                <p className="text-xs text-gray-600 mt-2">
                  {roleForm.default_policy === 'allow'
                    ? 'All tools are allowed unless a deny policy explicitly blocks them.'
                    : 'All tools are blocked unless an allow policy explicitly grants access.'}
                </p>
              </div>
              {error && (
                <div className="px-3 py-2 bg-danger/10 border border-danger/20 rounded-lg">
                  <p className="text-xs text-danger">{error}</p>
                </div>
              )}
              <div className="flex gap-3 pt-2">
                <button
                  onClick={() => setShowRoleModal(false)}
                  className="flex-1 py-2 bg-surface-hover border border-border text-gray-300 text-sm rounded-lg hover:bg-surface-active transition-colors"
                >
                  Cancel
                </button>
                <button
                  onClick={saveRole}
                  disabled={isSubmitting}
                  className="flex-1 py-2 bg-accent hover:bg-accent-hover text-white text-sm font-medium rounded-lg transition-colors disabled:opacity-50"
                >
                  {isSubmitting ? 'Saving…' : `${editingRole ? 'Update' : 'Create'} Role`}
                </button>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* Create user modal */}
      {showCreateModal && (
        <div className="fixed inset-0 bg-black/60 flex items-center justify-center z-50">
          <div className="bg-surface border border-border rounded-2xl p-6 w-full max-w-md">
            <div className="flex items-center justify-between mb-5">
              <h3 className="text-base font-semibold text-white">Create User</h3>
              <button onClick={() => setShowCreateModal(false)} className="text-gray-500 hover:text-gray-300">
                <X className="w-5 h-5" />
              </button>
            </div>

            <div className="space-y-4">
              <div>
                <label className="block text-xs font-medium text-gray-400 mb-1.5 uppercase tracking-wider">Username</label>
                <input
                  type="text"
                  value={newUser.username}
                  onChange={e => setNewUser({ ...newUser, username: e.target.value })}
                  className="w-full px-3 py-2 bg-[#0a0a0f] border border-border rounded-lg text-sm text-white focus:outline-none focus:border-accent/50"
                  placeholder="Enter username"
                />
              </div>
              <div>
                <label className="block text-xs font-medium text-gray-400 mb-1.5 uppercase tracking-wider">Password</label>
                <input
                  type="password"
                  value={newUser.password}
                  onChange={e => setNewUser({ ...newUser, password: e.target.value })}
                  className="w-full px-3 py-2 bg-[#0a0a0f] border border-border rounded-lg text-sm text-white focus:outline-none focus:border-accent/50"
                  placeholder="Enter password"
                />
              </div>
              <div>
                <label className="block text-xs font-medium text-gray-400 mb-1.5 uppercase tracking-wider">Email</label>
                <input
                  type="email"
                  value={newUser.email}
                  onChange={e => setNewUser({ ...newUser, email: e.target.value })}
                  className="w-full px-3 py-2 bg-[#0a0a0f] border border-border rounded-lg text-sm text-white focus:outline-none focus:border-accent/50"
                  placeholder="Optional email"
                />
              </div>
              <div>
                <label className="block text-xs font-medium text-gray-400 mb-1.5 uppercase tracking-wider">Role</label>
                <select
                  value={newUser.role}
                  onChange={e => setNewUser({ ...newUser, role: e.target.value })}
                  className="w-full px-3 py-2 bg-[#0a0a0f] border border-border rounded-lg text-sm text-gray-300 focus:outline-none focus:border-accent/50"
                >
                  {roles.map(r => <option key={r.role_id} value={r.name}>{r.name}</option>)}
                </select>
              </div>

              {error && (
                <div className="px-3 py-2 bg-danger/10 border border-danger/20 rounded-lg">
                  <p className="text-xs text-danger">{error}</p>
                </div>
              )}

              <div className="flex gap-3 pt-2">
                <button
                  onClick={() => setShowCreateModal(false)}
                  className="flex-1 py-2 bg-surface-hover border border-border text-gray-300 text-sm rounded-lg hover:bg-surface-active transition-colors"
                >
                  Cancel
                </button>
                <button
                  onClick={createUser}
                  disabled={isSubmitting}
                  className="flex-1 py-2 bg-accent hover:bg-accent-hover text-white text-sm font-medium rounded-lg transition-colors disabled:opacity-50"
                >
                  {isSubmitting ? 'Creating…' : 'Create User'}
                </button>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* Change password modal */}
      {showPasswordModal && (
        <div className="fixed inset-0 bg-black/60 flex items-center justify-center z-50">
          <div className="bg-surface border border-border rounded-2xl p-6 w-full max-w-md">
            <div className="flex items-center justify-between mb-5">
              <div>
                <h3 className="text-base font-semibold text-white">Change Password</h3>
                <p className="text-xs text-gray-500 mt-0.5">for {passwordUsername}</p>
              </div>
              <button onClick={() => setShowPasswordModal(false)} className="text-gray-500 hover:text-gray-300">
                <X className="w-5 h-5" />
              </button>
            </div>

            {passwordSuccess ? (
              <div className="flex items-center gap-3 py-8 justify-center">
                <Check className="w-6 h-6 text-success" />
                <span className="text-sm text-success font-medium">Password updated successfully</span>
              </div>
            ) : (
              <div className="space-y-4">
                <div>
                  <label className="block text-xs font-medium text-gray-400 mb-1.5 uppercase tracking-wider">New Password</label>
                  <input
                    type="password"
                    value={newPassword}
                    onChange={e => setNewPassword(e.target.value)}
                    className="w-full px-3 py-2 bg-[#0a0a0f] border border-border rounded-lg text-sm text-white focus:outline-none focus:border-accent/50"
                    placeholder="Enter new password"
                  />
                </div>
                <div>
                  <label className="block text-xs font-medium text-gray-400 mb-1.5 uppercase tracking-wider">Confirm Password</label>
                  <input
                    type="password"
                    value={confirmPassword}
                    onChange={e => setConfirmPassword(e.target.value)}
                    className="w-full px-3 py-2 bg-[#0a0a0f] border border-border rounded-lg text-sm text-white focus:outline-none focus:border-accent/50"
                    placeholder="Confirm new password"
                  />
                </div>

                {error && (
                  <div className="px-3 py-2 bg-danger/10 border border-danger/20 rounded-lg">
                    <p className="text-xs text-danger">{error}</p>
                  </div>
                )}

                <div className="flex gap-3 pt-2">
                  <button
                    onClick={() => setShowPasswordModal(false)}
                    className="flex-1 py-2 bg-surface-hover border border-border text-gray-300 text-sm rounded-lg hover:bg-surface-active transition-colors"
                  >
                    Cancel
                  </button>
                  <button
                    onClick={changePassword}
                    disabled={isSubmitting}
                    className="flex-1 py-2 bg-accent hover:bg-accent-hover text-white text-sm font-medium rounded-lg transition-colors disabled:opacity-50"
                  >
                    {isSubmitting ? 'Saving…' : 'Update Password'}
                  </button>
                </div>
              </div>
            )}
          </div>
        </div>
      )}

      {/* Delete confirmation dialog */}
      {deleteConfirm && (
        <div className="fixed inset-0 bg-black/60 flex items-center justify-center z-50">
          <div className="bg-surface border border-border rounded-2xl p-6 w-full max-w-md">
            <div className="flex items-center gap-3 mb-4">
              <div className="w-10 h-10 rounded-xl bg-danger/10 flex items-center justify-center">
                <AlertTriangle className="w-5 h-5 text-danger" />
              </div>
              <div>
                <h3 className="text-base font-semibold text-white">
                  Delete {deleteConfirm.type === 'role' ? 'Role' : 'User'}
                </h3>
                <p className="text-sm text-gray-400">
                  This action cannot be undone.
                </p>
              </div>
            </div>

            <div className="space-y-3 mb-5">
              <p className="text-sm text-gray-300">
                Are you sure you want to delete <span className="font-semibold text-white">{deleteConfirm.name}</span>?
              </p>

              {deleteConfirm.type === 'role' && deleteConfirm.impact && (
                <div className="bg-[#0a0a0f] border border-border rounded-lg p-3 space-y-2">
                  <p className="text-xs font-medium text-gray-400 uppercase tracking-wider">Cascading Impact</p>

                  <div className="flex items-center justify-between text-sm">
                    <span className="text-gray-400">User assignments removed</span>
                    <span className="text-white font-medium">{deleteConfirm.impact.affected_user_count}</span>
                  </div>

                  <div className="flex items-center justify-between text-sm">
                    <span className="text-gray-400">Policy bindings removed</span>
                    <span className="text-white font-medium">{deleteConfirm.impact.policy_binding_count}</span>
                  </div>

                  {deleteConfirm.impact.orphaned_users.length > 0 && (
                    <div className="mt-2 px-3 py-2 bg-yellow-500/10 border border-yellow-500/20 rounded-lg">
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
                    <div className="text-xs text-gray-500 mt-1">
                      Affected users: {deleteConfirm.impact.affected_users.join(', ')}
                    </div>
                  )}
                </div>
              )}

              {deleteConfirm.type === 'user' && (
                <div className="bg-[#0a0a0f] border border-border rounded-lg p-3 space-y-1">
                  <p className="text-xs font-medium text-gray-400 uppercase tracking-wider">What will be removed</p>
                  <ul className="text-sm text-gray-400 space-y-0.5">
                    <li>All role assignments for this user</li>
                    <li>All API keys belonging to this user</li>
                    <li>Policies created by this user will retain but lose their creator reference</li>
                  </ul>
                </div>
              )}
            </div>

            <div className="flex gap-3">
              <button
                onClick={() => setDeleteConfirm(null)}
                disabled={deleteLoading}
                className="flex-1 py-2 bg-surface-hover border border-border text-gray-300 text-sm rounded-lg hover:bg-surface-active transition-colors"
              >
                Cancel
              </button>
              <button
                onClick={executeDelete}
                disabled={deleteLoading}
                className="flex-1 py-2 bg-danger hover:bg-danger/80 text-white text-sm font-medium rounded-lg transition-colors disabled:opacity-50"
              >
                {deleteLoading ? 'Deleting...' : 'Delete'}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
