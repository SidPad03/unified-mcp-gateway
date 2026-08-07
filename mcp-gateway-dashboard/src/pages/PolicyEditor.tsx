import { useState, useEffect, useRef } from 'react';
import { api, Policy, Role } from '@/lib/api';
import { Edit3, GripVertical, Plus, Shield, Trash2 } from 'lucide-react';
import clsx from 'clsx';
import { SUPPORTED_APPS, APP_LABELS, type AppSlug } from '@/lib/connectors';
import {
  Badge,
  Banner,
  Button,
  Card,
  ConfirmModal,
  EmptyState,
  Field,
  IconButton,
  Input,
  Label,
  Loading,
  Modal,
  Mono,
  PageHeader,
  RISK_LEVELS,
  RiskBadge,
  Select,
  Toggle,
  Tone,
} from '@/components/ui';

const DECISION: Record<string, { tone: Tone; label: string }> = {
  allow: { tone: 'ok', label: 'Allow' },
  deny: { tone: 'deny', label: 'Deny' },
  require_approval: { tone: 'warn', label: 'Require approval' },
  conditional: { tone: 'warn', label: 'Conditional' },
};

export default function PolicyEditor() {
  const [policies, setPolicies] = useState<Policy[]>([]);
  const [roles, setRoles] = useState<Role[]>([]);
  const [loading, setLoading] = useState(true);
  const [showModal, setShowModal] = useState(false);
  const [editingPolicy, setEditingPolicy] = useState<Policy | null>(null);
  const [deleting, setDeleting] = useState<Policy | null>(null);
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
      const [policyData, roleData] = await Promise.all([api.getPolicies(), api.getRoles()]);
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
    setForm({
      name: '',
      priority: 0,
      decision: 'deny',
      reason: '',
      tool_pattern: '*',
      role_ids: [],
      risk_categories: [],
      application_match: '',
    });
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

  const deletePolicy = async () => {
    if (!deleting) return;
    try {
      await api.deletePolicy(deleting.policy_id);
      setDeleting(null);
      loadData();
    } catch (e: any) {
      setPageError(e.message || 'Failed to delete policy');
      setDeleting(null);
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

  const toggleRole = (roleId: string) =>
    setForm(prev => ({
      ...prev,
      role_ids: prev.role_ids.includes(roleId)
        ? prev.role_ids.filter(id => id !== roleId)
        : [...prev.role_ids, roleId],
    }));

  const toggleRisk = (cat: string) =>
    setForm(prev => ({
      ...prev,
      risk_categories: prev.risk_categories.includes(cat)
        ? prev.risk_categories.filter(c => c !== cat)
        : [...prev.risk_categories, cat],
    }));

  const handleDrop = async (fromIdx: number, toIdx: number) => {
    if (fromIdx === toIdx) return;
    const reordered = [...policies];
    const [moved] = reordered.splice(fromIdx, 1);
    reordered.splice(toIdx, 0, moved);
    // Optimistic update
    setPolicies(reordered.map((p, i) => ({ ...p, priority: i + 1 })));
    try {
      // Two passes: priorities are unique, so park everything out of the way
      // before assigning the final values, or the first write collides.
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
      <PageHeader
        title="Policies"
        description="Rules are evaluated in order and the first match wins, so the row above always beats the row below. Drag to reorder."
        actions={
          <Button variant="primary" icon={Plus} onClick={openCreate}>
            Add policy
          </Button>
        }
      />

      {pageError && (
        <Banner tone="deny" onDismiss={() => setPageError('')} className="mb-4">
          {pageError}
        </Banner>
      )}

      {loading ? (
        <Card>
          <Loading label="Loading policies..." />
        </Card>
      ) : policies.length === 0 ? (
        <Card>
          <EmptyState
            icon={Shield}
            title="No policies defined"
            message="Without a policy, every role falls back to its default decision. Add a rule to constrain what the AI clients can reach."
            action={
              <Button variant="primary" icon={Plus} onClick={openCreate}>
                Add policy
              </Button>
            }
          />
        </Card>
      ) : (
        <div className="bg-panel border border-line rounded-card shadow-[var(--shadow-card)] overflow-hidden">
          {policies.map((policy, idx) => {
            const decision = DECISION[policy.decision] || DECISION.allow;
            // A disabled rule is not evaluated, so its rail goes dark rather
            // than showing a decision it is not making.
            const tone: Tone = policy.is_active ? decision.tone : 'neutral';
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
                  'grid grid-cols-[3px_1fr] border-b border-line-soft last:border-b-0',
                  'transition-[opacity,background-color] duration-150',
                  !policy.is_active && 'opacity-55',
                  dragIdx === idx && 'opacity-35',
                  dropIdx === idx && dragIdx !== idx && 'bg-beam-wash'
                )}
              >
                <div
                  style={{
                    background:
                      tone === 'ok'
                        ? 'var(--beam)'
                        : tone === 'deny'
                          ? 'var(--deny)'
                          : tone === 'warn'
                            ? 'var(--warn)'
                            : 'var(--line-strong)',
                  }}
                />
                <div className="flex items-start gap-3 px-3.5 py-3">
                  <div className="flex items-center gap-2 pt-0.5 cursor-grab active:cursor-grabbing shrink-0">
                    <GripVertical className="w-3.5 h-3.5 text-ink-4" />
                    <Mono className="text-2xs text-ink-4 w-6">{policy.priority}</Mono>
                  </div>

                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2 flex-wrap">
                      <span className="text-xs font-medium text-ink">{policy.name}</span>
                      <Badge tone={decision.tone} dot>
                        {decision.label}
                      </Badge>
                      {!policy.is_active && <Badge>disabled</Badge>}
                    </div>

                    <div className="flex items-center gap-1.5 mt-1.5 flex-wrap">
                      <code className="text-micro font-mono text-ink-2 bg-inset border border-line px-1.5 py-0.5 rounded-[4px]">
                        {policy.tool_pattern}
                      </code>
                      {policy.risk_categories?.length > 0 ? (
                        policy.risk_categories.map(cat => <RiskBadge key={cat} risk={cat} />)
                      ) : (
                        <span className="text-micro text-ink-4 italic">all risks</span>
                      )}
                      {policy.application_match && (
                        <Badge>
                          {APP_LABELS[policy.application_match as AppSlug] ||
                            policy.application_match}
                        </Badge>
                      )}
                    </div>

                    {policy.reason && (
                      <p className="text-2xs text-ink-3 mt-1.5">{policy.reason}</p>
                    )}

                    {policy.role_names?.length > 0 && (
                      <div className="flex items-center gap-1.5 mt-2 flex-wrap">
                        <Label>Roles</Label>
                        {policy.role_names.map(role => (
                          <Badge key={role}>{role}</Badge>
                        ))}
                      </div>
                    )}
                  </div>

                  <div className="flex items-center gap-1 shrink-0">
                    <Toggle
                      checked={policy.is_active}
                      onChange={() => togglePolicy(policy)}
                      label={`${policy.is_active ? 'Disable' : 'Enable'} ${policy.name}`}
                    />
                    <IconButton icon={Edit3} label="Edit policy" onClick={() => openEdit(policy)} />
                    <IconButton
                      icon={Trash2}
                      label="Delete policy"
                      onClick={() => setDeleting(policy)}
                      className="hover:text-deny"
                    />
                  </div>
                </div>
              </div>
            );
          })}
        </div>
      )}

      <Modal
        open={showModal}
        onClose={() => setShowModal(false)}
        title={editingPolicy ? 'Edit policy' : 'Create policy'}
        description="A policy matches on tool pattern, risk level and application, then allows or denies."
        footer={
          <>
            <Button variant="ghost" onClick={() => setShowModal(false)}>
              Cancel
            </Button>
            <Button variant="primary" onClick={savePolicy} loading={isSubmitting}>
              {editingPolicy ? 'Save changes' : 'Create policy'}
            </Button>
          </>
        }
      >
        <div className="space-y-4">
          <div className={clsx('grid gap-3', editingPolicy ? 'grid-cols-[1fr_100px]' : 'grid-cols-1')}>
            <Field label="Name" hint={editingPolicy ? undefined : 'Priority is assigned automatically.'}>
              <Input
                value={form.name}
                onChange={e => setForm({ ...form, name: e.target.value })}
                placeholder="Block destructive tools for viewers"
                autoFocus
              />
            </Field>
            {editingPolicy && (
              <Field label="Priority" hint="Must be unique.">
                <Input
                  type="number"
                  min={1}
                  value={form.priority}
                  onChange={e => setForm({ ...form, priority: parseInt(e.target.value) || 1 })}
                />
              </Field>
            )}
          </div>

          <Field
            label="Tool pattern"
            hint={
              <>
                Glob syntax. <Mono>*</Mono> matches everything, <Mono>github__*</Mono> one backend,{' '}
                <Mono>*__delete_*</Mono> a verb.
              </>
            }
          >
            <Input
              value={form.tool_pattern}
              onChange={e => setForm({ ...form, tool_pattern: e.target.value })}
              placeholder="*"
              className="font-mono"
            />
          </Field>

          <Field
            label="Application"
            hint="Restrict this rule to one AI client, or leave it matching every app."
          >
            <Select
              value={form.application_match}
              onChange={e => setForm({ ...form, application_match: e.target.value })}
              className="w-full"
            >
              <option value="">All applications</option>
              {SUPPORTED_APPS.map(app => (
                <option key={app} value={app}>
                  {APP_LABELS[app]}
                </option>
              ))}
            </Select>
          </Field>

          <Field
            label="Risk levels"
            hint={
              form.risk_categories.length === 0
                ? 'None selected. The rule matches every risk level.'
                : undefined
            }
          >
            <div className="flex flex-wrap gap-1.5">
              {RISK_LEVELS.map(cat => (
                <RiskBadge
                  key={cat}
                  risk={cat}
                  active={form.risk_categories.includes(cat)}
                  onClick={() => toggleRisk(cat)}
                  className={clsx(!form.risk_categories.includes(cat) && 'opacity-50')}
                />
              ))}
            </div>
          </Field>

          <Field label="Decision">
            <Select
              value={form.decision}
              onChange={e => setForm({ ...form, decision: e.target.value })}
              className="w-full"
            >
              <option value="allow">Allow</option>
              <option value="deny">Deny</option>
            </Select>
          </Field>

          <Field label="Reason" hint="Shown in the audit trail when this rule is what stopped a call.">
            <Input
              value={form.reason}
              onChange={e => setForm({ ...form, reason: e.target.value })}
              placeholder="Destructive tools require an approved client"
            />
          </Field>

          <Field label="Roles" hint="Which roles this rule is bound to.">
            <div className="space-y-1.5 mt-1">
              {roles.map(role => (
                <label
                  key={role.role_id}
                  className="flex items-center gap-2.5 cursor-pointer py-0.5 group"
                >
                  <input
                    type="checkbox"
                    checked={form.role_ids.includes(role.role_id)}
                    onChange={() => toggleRole(role.role_id)}
                  />
                  <span className="text-xs text-ink-2 group-hover:text-ink transition-colors">
                    {role.name}
                  </span>
                  {role.is_system && <Badge>system</Badge>}
                </label>
              ))}
              {roles.length === 0 && <p className="text-2xs text-ink-4">No roles available.</p>}
            </div>
          </Field>

          {error && <Banner tone="deny">{error}</Banner>}
        </div>
      </Modal>

      <ConfirmModal
        open={!!deleting}
        onClose={() => setDeleting(null)}
        onConfirm={deletePolicy}
        title="Delete this policy?"
        description="Calls that matched it will fall through to the next rule, or to the role's default decision."
        confirmLabel="Delete policy"
      >
        {deleting && (
          <p className="text-xs text-ink-2">
            <span className="text-ink font-medium">{deleting.name}</span> ·{' '}
            <Mono className="text-ink-3">{deleting.tool_pattern}</Mono>
          </p>
        )}
      </ConfirmModal>
    </div>
  );
}
