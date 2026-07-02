import { useState, useEffect, useRef } from 'react';
import { api, Policy, Role } from '@/lib/api';
import { Plus, Trash2, Edit3, X, ShieldCheck, ShieldOff, GripVertical } from 'lucide-react';
import clsx from 'clsx';
import { SUPPORTED_APPS, APP_LABELS, type AppSlug } from '@/lib/connectors';

const DECISION_CONFIG: Record<string, { icon: typeof ShieldCheck; color: string; bg: string; label: string }> = {
  allow: { icon: ShieldCheck, color: 'text-success', bg: 'bg-success/10', label: 'Allow' },
  deny: { icon: ShieldOff, color: 'text-danger', bg: 'bg-danger/10', label: 'Deny' },
};

const ALL_RISK_CATEGORIES = ['read', 'write', 'admin', 'destructive', 'execute', 'unclassified'];

const RISK_COLORS: Record<string, string> = {
  read: 'bg-emerald-500/10 text-emerald-400 border-emerald-500/20',
  write: 'bg-blue-500/10 text-blue-400 border-blue-500/20',
  admin: 'bg-orange-500/10 text-orange-400 border-orange-500/20',
  destructive: 'bg-red-500/10 text-red-400 border-red-500/20',
  execute: 'bg-purple-500/10 text-purple-400 border-purple-500/20',
  unclassified: 'bg-gray-500/10 text-gray-400 border-gray-500/20',
};

export default function PolicyEditor() {
  const [policies, setPolicies] = useState<Policy[]>([]);
  const [roles, setRoles] = useState<Role[]>([]);
  const [loading, setLoading] = useState(true);
  const [showModal, setShowModal] = useState(false);
  const [editingPolicy, setEditingPolicy] = useState<Policy | null>(null);
  const [form, setForm] = useState({
    name: '',
    priority: 100,
    decision: 'deny',
    reason: '',
    tool_pattern: '*',
    role_ids: [] as string[],
    risk_categories: [] as string[],
    application_match: '',
  });
  const [error, setError] = useState('');
  const [pageError, setPageError] = useState('');
  const [dragIdx, setDragIdx] = useState<number | null>(null);
  const [dropIdx, setDropIdx] = useState<number | null>(null);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const dragCounter = useRef(0);

  useEffect(() => {
    loadData();
  }, []);

  const loadData = async () => {
    try {
      const [policyData, roleData] = await Promise.all([
        api.getPolicies(),
        api.getRoles(),
      ]);
      setPolicies(policyData);
      setRoles(roleData);
      setPageError('');
    } catch (e: any) {
      setPageError(e.message || 'Failed to load data');
    } finally {
      setLoading(false);
    }
  };

  const openCreate = () => {
    setEditingPolicy(null);
    setForm({ name: '', priority: 0, decision: 'deny', reason: '', tool_pattern: '*', role_ids: [], risk_categories: [], application_match: '' });
    setError('');
    setShowModal(true);
  };

  const openEdit = (policy: Policy) => {
    setEditingPolicy(policy);
    setForm({
      name: policy.name,
      priority: policy.priority,
      decision: policy.decision,
      reason: policy.reason || '',
      tool_pattern: policy.tool_pattern,
      role_ids: policy.role_ids || [],
      risk_categories: policy.risk_categories || [],
      application_match: policy.application_match || '',
    });
    setError('');
    setShowModal(true);
  };

  const savePolicy = async () => {
    if (isSubmitting) return;
    setError('');
    setIsSubmitting(true);
    try {
      if (editingPolicy) {
        await api.updatePolicy(editingPolicy.policy_id, {
          name: form.name,
          priority: form.priority,
          decision: form.decision,
          reason: form.reason || undefined,
          tool_pattern: form.tool_pattern,
          role_ids: form.role_ids,
          risk_categories: form.risk_categories,
          application_match: form.application_match || undefined,
        });
      } else {
        await api.createPolicy({
          name: form.name,
          decision: form.decision,
          reason: form.reason || undefined,
          tool_pattern: form.tool_pattern,
          role_ids: form.role_ids,
          risk_categories: form.risk_categories,
          application_match: form.application_match || undefined,
        });
      }
      setShowModal(false);
      loadData();
    } catch (e: any) {
      setError(e.message || 'Failed to save policy');
    } finally {
      setIsSubmitting(false);
    }
  };

  const deletePolicy = async (id: string) => {
    try {
      await api.deletePolicy(id);
      loadData();
    } catch (e: any) {
      setPageError(e.message || 'Failed to delete policy');
    }
  };

  const togglePolicy = async (policy: Policy) => {
    try {
      await api.updatePolicy(policy.policy_id, { is_active: !policy.is_active });
      loadData();
    } catch (e: any) {
      setPageError(e.message || 'Failed to toggle policy');
    }
  };

  const toggleRole = (roleId: string) => {
    setForm(prev => ({
      ...prev,
      role_ids: prev.role_ids.includes(roleId)
        ? prev.role_ids.filter(id => id !== roleId)
        : [...prev.role_ids, roleId],
    }));
  };

  const toggleRisk = (cat: string) => {
    setForm(prev => ({
      ...prev,
      risk_categories: prev.risk_categories.includes(cat)
        ? prev.risk_categories.filter(c => c !== cat)
        : [...prev.risk_categories, cat],
    }));
  };

  const handleDrop = async (fromIdx: number, toIdx: number) => {
    if (fromIdx === toIdx) return;
    const reordered = [...policies];
    const [moved] = reordered.splice(fromIdx, 1);
    reordered.splice(toIdx, 0, moved);
    // Reassign priorities sequentially
    const updates: Promise<unknown>[] = [];
    reordered.forEach((p, i) => {
      const newPriority = i + 1;
      if (p.priority !== newPriority) {
        updates.push(api.updatePolicy(p.policy_id, { priority: newPriority }));
      }
    });
    // Optimistic update
    setPolicies(reordered.map((p, i) => ({ ...p, priority: i + 1 })));
    try {
      // Update one at a time to avoid priority conflicts
      for (const p of reordered) {
        const newPriority = reordered.indexOf(p) + 1;
        if (p.priority !== newPriority) {
          await api.updatePolicy(p.policy_id, { priority: newPriority + 1000 });
        }
      }
      for (let i = 0; i < reordered.length; i++) {
        await api.updatePolicy(reordered[i].policy_id, { priority: i + 1 });
      }
      loadData();
    } catch (e: any) {
      setPageError(e.message || 'Failed to reorder policies');
      loadData();
    }
  };

  return (
    <div>
      <div className="flex items-center justify-between mb-6">
        <div>
          <h2 className="text-lg font-semibold text-white">Policy Rules</h2>
          <p className="text-sm text-gray-500 mt-1">
            Policies are bound to roles. Lower priority = checked first. Filter by tool pattern and/or risk level.
          </p>
        </div>
        <button
          onClick={openCreate}
          className="flex items-center gap-2 px-4 py-2 bg-accent hover:bg-accent-hover text-white text-sm font-medium rounded-lg transition-colors"
        >
          <Plus className="w-4 h-4" />
          Add Policy
        </button>
      </div>

      {pageError && (
        <div className="mb-4 px-4 py-3 bg-danger/10 border border-danger/20 rounded-lg flex items-center justify-between">
          <p className="text-sm text-danger">{pageError}</p>
          <button onClick={() => setPageError('')} className="text-danger/60 hover:text-danger">
            <X className="w-4 h-4" />
          </button>
        </div>
      )}

      {/* Policy list */}
      <div className="space-y-2">
        {loading ? (
          <div className="bg-surface border border-border rounded-xl p-12 text-center text-gray-500 text-sm">Loading policies...</div>
        ) : policies.length === 0 ? (
          <div className="bg-surface border border-border rounded-xl p-12 text-center text-gray-500 text-sm">No policies defined</div>
        ) : (
          policies.map((policy, idx) => {
            const config = DECISION_CONFIG[policy.decision] || DECISION_CONFIG.allow;
            const Icon = config.icon;
            return (
              <div
                key={policy.policy_id}
                draggable
                onDragStart={e => {
                  setDragIdx(idx);
                  e.dataTransfer.effectAllowed = 'move';
                  e.dataTransfer.setData('text/plain', String(idx));
                }}
                onDragEnter={e => {
                  e.preventDefault();
                  dragCounter.current++;
                  setDropIdx(idx);
                }}
                onDragLeave={() => {
                  dragCounter.current--;
                  if (dragCounter.current === 0) setDropIdx(null);
                }}
                onDragOver={e => {
                  e.preventDefault();
                  e.dataTransfer.dropEffect = 'move';
                }}
                onDrop={e => {
                  e.preventDefault();
                  dragCounter.current = 0;
                  const from = parseInt(e.dataTransfer.getData('text/plain'), 10);
                  setDragIdx(null);
                  setDropIdx(null);
                  handleDrop(from, idx);
                }}
                onDragEnd={() => {
                  setDragIdx(null);
                  setDropIdx(null);
                  dragCounter.current = 0;
                }}
                className={clsx(
                  'bg-surface border rounded-xl p-4 transition-all',
                  policy.is_active ? 'border-border' : 'border-border/50 opacity-60',
                  dragIdx === idx && 'opacity-40',
                  dropIdx === idx && dragIdx !== idx && 'border-accent/50 ring-1 ring-accent/20'
                )}
              >
                <div className="flex items-start gap-4">
                  <div className="flex items-center gap-2 mt-0.5 cursor-grab active:cursor-grabbing">
                    <GripVertical className="w-4 h-4 text-gray-600 hover:text-gray-400" />
                    <span className="text-xs text-gray-500 font-mono w-8">#{policy.priority}</span>
                  </div>

                  <div className={clsx('w-8 h-8 rounded-lg flex items-center justify-center shrink-0', config.bg)}>
                    <Icon className={clsx('w-4 h-4', config.color)} />
                  </div>

                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2 flex-wrap">
                      <span className="text-sm font-medium text-white">{policy.name}</span>
                      <span className={clsx('text-xs font-medium px-1.5 py-0.5 rounded', config.bg, config.color)}>
                        {config.label}
                      </span>
                      {!policy.is_active && (
                        <span className="text-xs text-gray-500 bg-surface-active px-1.5 py-0.5 rounded">disabled</span>
                      )}
                    </div>
                    <div className="flex items-center gap-2 mt-1 flex-wrap">
                      <code className="text-xs text-accent bg-accent/10 px-1.5 py-0.5 rounded font-mono">
                        {policy.tool_pattern}
                      </code>
                      {policy.risk_categories && policy.risk_categories.length > 0 && (
                        policy.risk_categories.map(cat => (
                          <span key={cat} className={clsx('text-xs px-1.5 py-0.5 rounded-full border', RISK_COLORS[cat] || RISK_COLORS.unclassified)}>
                            {cat}
                          </span>
                        ))
                      )}
                      {(!policy.risk_categories || policy.risk_categories.length === 0) && (
                        <span className="text-xs text-gray-600 italic">all risks</span>
                      )}
                      {policy.application_match && (
                        <span className="text-xs text-violet-400 bg-violet-500/10 px-1.5 py-0.5 rounded-full border border-violet-500/20">
                          app: {APP_LABELS[policy.application_match as AppSlug] || policy.application_match}
                        </span>
                      )}
                    </div>
                    {policy.reason && (
                      <p className="text-xs text-gray-500 mt-1">{policy.reason}</p>
                    )}
                    {policy.role_names && policy.role_names.length > 0 && (
                      <div className="flex gap-1 mt-2 flex-wrap">
                        {policy.role_names.map(role => (
                          <span key={role} className="text-xs text-blue-400 bg-blue-500/10 px-1.5 py-0.5 rounded">
                            {role}
                          </span>
                        ))}
                      </div>
                    )}
                  </div>

                  <div className="flex items-center gap-1 shrink-0">
                    <button
                      onClick={() => togglePolicy(policy)}
                      className={clsx('text-xs px-2 py-1 rounded transition-colors',
                        policy.is_active ? 'text-gray-400 hover:bg-surface-hover' : 'text-success hover:bg-success/10'
                      )}
                    >
                      {policy.is_active ? 'Disable' : 'Enable'}
                    </button>
                    <button
                      onClick={() => openEdit(policy)}
                      className="p-1.5 text-gray-500 hover:text-gray-300 hover:bg-surface-hover rounded-md transition-colors"
                    >
                      <Edit3 className="w-3.5 h-3.5" />
                    </button>
                    <button
                      onClick={() => deletePolicy(policy.policy_id)}
                      className="p-1.5 text-gray-500 hover:text-danger hover:bg-danger/10 rounded-md transition-colors"
                    >
                      <Trash2 className="w-3.5 h-3.5" />
                    </button>
                  </div>
                </div>
              </div>
            );
          })
        )}
      </div>

      {/* Create/Edit modal */}
      {showModal && (
        <div className="fixed inset-0 bg-black/60 flex items-center justify-center z-50">
          <div className="bg-surface border border-border rounded-2xl p-6 w-full max-w-lg">
            <div className="flex items-center justify-between mb-5">
              <h3 className="text-base font-semibold text-white">
                {editingPolicy ? 'Edit Policy' : 'Create Policy'}
              </h3>
              <button onClick={() => setShowModal(false)} className="text-gray-500 hover:text-gray-300">
                <X className="w-5 h-5" />
              </button>
            </div>

            <div className="space-y-4">
              {editingPolicy ? (
                <div className="grid grid-cols-2 gap-4">
                  <div>
                    <label className="block text-xs font-medium text-gray-400 mb-1.5 uppercase tracking-wider">Name</label>
                    <input
                      type="text"
                      value={form.name}
                      onChange={e => setForm({ ...form, name: e.target.value })}
                      className="w-full px-3 py-2 bg-[#0a0a0f] border border-border rounded-lg text-sm text-white focus:outline-none focus:border-accent/50"
                      placeholder="Policy name"
                    />
                  </div>
                  <div>
                    <label className="block text-xs font-medium text-gray-400 mb-1.5 uppercase tracking-wider">Priority</label>
                    <input
                      type="number"
                      min={1}
                      value={form.priority}
                      onChange={e => setForm({ ...form, priority: parseInt(e.target.value) || 1 })}
                      className="w-full px-3 py-2 bg-[#0a0a0f] border border-border rounded-lg text-sm text-white focus:outline-none focus:border-accent/50"
                    />
                    <p className="text-xs text-gray-600 mt-1">Must be unique across all policies</p>
                  </div>
                </div>
              ) : (
                <div>
                  <label className="block text-xs font-medium text-gray-400 mb-1.5 uppercase tracking-wider">Name</label>
                  <input
                    type="text"
                    value={form.name}
                    onChange={e => setForm({ ...form, name: e.target.value })}
                    className="w-full px-3 py-2 bg-[#0a0a0f] border border-border rounded-lg text-sm text-white focus:outline-none focus:border-accent/50"
                    placeholder="Policy name"
                  />
                  <p className="text-xs text-gray-600 mt-1">Priority is auto-assigned</p>
                </div>
              )}

              <div>
                <label className="block text-xs font-medium text-gray-400 mb-1.5 uppercase tracking-wider">Tool Pattern</label>
                <input
                  type="text"
                  value={form.tool_pattern}
                  onChange={e => setForm({ ...form, tool_pattern: e.target.value })}
                  className="w-full px-3 py-2 bg-[#0a0a0f] border border-border rounded-lg text-sm text-white font-mono focus:outline-none focus:border-accent/50"
                  placeholder="filesystem.*"
                />
                <p className="text-xs text-gray-600 mt-1">
                  Glob syntax: <code className="text-gray-500">*</code> matches all, <code className="text-gray-500">backend.*</code> matches a backend, <code className="text-gray-500">*.read_*</code> matches pattern
                </p>
              </div>

              <div>
                <label className="block text-xs font-medium text-gray-400 mb-1.5 uppercase tracking-wider">Application Match</label>
                <select
                  value={form.application_match}
                  onChange={e => setForm({ ...form, application_match: e.target.value })}
                  className="w-full px-3 py-2 bg-[#0a0a0f] border border-border rounded-lg text-sm text-gray-300 focus:outline-none focus:border-accent/50"
                >
                  <option value="">All applications</option>
                  {SUPPORTED_APPS.map(app => (
                    <option key={app} value={app}>{APP_LABELS[app]}</option>
                  ))}
                </select>
                <p className="text-xs text-gray-600 mt-1">
                  Restrict this policy to a specific AI client. Leave as "All" to match every app.
                </p>
              </div>

              <div>
                <label className="block text-xs font-medium text-gray-400 mb-1.5 uppercase tracking-wider">Risk Levels</label>
                <p className="text-xs text-gray-600 mb-2">
                  Select which risk levels this policy applies to. Leave empty to match all risk levels.
                </p>
                <div className="flex flex-wrap gap-2">
                  {ALL_RISK_CATEGORIES.map(cat => (
                    <button
                      key={cat}
                      type="button"
                      onClick={() => toggleRisk(cat)}
                      className={clsx(
                        'px-2.5 py-1 text-xs font-medium rounded-full border transition-colors',
                        form.risk_categories.includes(cat)
                          ? RISK_COLORS[cat] + ' ring-1 ring-white/20'
                          : 'bg-[#0a0a0f] border-border text-gray-600 hover:border-border-hover'
                      )}
                    >
                      {cat}
                    </button>
                  ))}
                </div>
                {form.risk_categories.length === 0 && (
                  <p className="text-xs text-gray-600 mt-1.5 italic">All risk levels (no filter)</p>
                )}
              </div>

              <div>
                <label className="block text-xs font-medium text-gray-400 mb-1.5 uppercase tracking-wider">Decision</label>
                <select
                  value={form.decision}
                  onChange={e => setForm({ ...form, decision: e.target.value })}
                  className="w-full px-3 py-2 bg-[#0a0a0f] border border-border rounded-lg text-sm text-gray-300 focus:outline-none focus:border-accent/50"
                >
                  <option value="allow">Allow</option>
                  <option value="deny">Deny</option>
                </select>
              </div>

              <div>
                <label className="block text-xs font-medium text-gray-400 mb-1.5 uppercase tracking-wider">Reason</label>
                <input
                  type="text"
                  value={form.reason}
                  onChange={e => setForm({ ...form, reason: e.target.value })}
                  className="w-full px-3 py-2 bg-[#0a0a0f] border border-border rounded-lg text-sm text-white focus:outline-none focus:border-accent/50"
                  placeholder="Human-readable reason for this policy"
                />
              </div>

              <div>
                <label className="block text-xs font-medium text-gray-400 mb-1.5 uppercase tracking-wider">Roles</label>
                <div className="space-y-2 mt-1">
                  {roles.map(role => (
                    <label key={role.role_id} className="flex items-center gap-2 cursor-pointer">
                      <input
                        type="checkbox"
                        checked={form.role_ids.includes(role.role_id)}
                        onChange={() => toggleRole(role.role_id)}
                        className="rounded border-border bg-[#0a0a0f]"
                      />
                      <span className="text-sm text-gray-300">{role.name}</span>
                      {role.is_system && (
                        <span className="text-xs text-gray-600">(system)</span>
                      )}
                    </label>
                  ))}
                  {roles.length === 0 && (
                    <p className="text-xs text-gray-600">No roles available</p>
                  )}
                </div>
              </div>

              {error && (
                <div className="px-3 py-2 bg-danger/10 border border-danger/20 rounded-lg">
                  <p className="text-xs text-danger">{error}</p>
                </div>
              )}

              <div className="flex gap-3 pt-2">
                <button
                  onClick={() => setShowModal(false)}
                  className="flex-1 py-2 bg-surface-hover border border-border text-gray-300 text-sm rounded-lg hover:bg-surface-active transition-colors"
                >
                  Cancel
                </button>
                <button
                  onClick={savePolicy}
                  disabled={isSubmitting}
                  className="flex-1 py-2 bg-accent hover:bg-accent-hover text-white text-sm font-medium rounded-lg transition-colors disabled:opacity-50"
                >
                  {isSubmitting ? 'Saving…' : `${editingPolicy ? 'Update' : 'Create'} Policy`}
                </button>
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
