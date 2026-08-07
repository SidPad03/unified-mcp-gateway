import { useState, FormEvent } from 'react';
import { api, User } from '@/lib/api';
import AuthShell from '@/components/AuthShell';
import { Banner, Button, Field, Mono, PasswordInput } from '@/components/ui';

interface Props {
  user: User;
  onComplete: (token?: string) => void;
  onLogout: () => void;
}

export default function ForcePasswordSetup({ user, onComplete, onLogout }: Props) {
  const [password, setPassword] = useState('');
  const [confirm, setConfirm] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [isSaving, setIsSaving] = useState(false);

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);

    if (password.length < 4) {
      setError('Password must be at least 4 characters');
      return;
    }
    if (password === 'admin') {
      setError('Choose a password other than the default');
      return;
    }
    if (password !== confirm) {
      setError('Passwords do not match');
      return;
    }

    setIsSaving(true);
    try {
      const res = await api.updateUser(user.user_id, { password });
      // Pass the re-issued token up so the session continues — the change just
      // revoked the token we logged in with; without it the next request 401s
      // and bounces to /login with no explanation.
      onComplete(res.token);
    } catch (err: any) {
      setError(err.message || 'Failed to set password');
    } finally {
      setIsSaving(false);
    }
  };

  return (
    <AuthShell
      title="Set your password"
      description={
        <>
          <Mono className="text-ink-2">{user.username}</Mono> is still on the default password.
          Choose a new one to continue.
        </>
      }
      footer={
        <button
          type="button"
          onClick={onLogout}
          className="text-2xs text-ink-4 hover:text-ink-2 transition-colors"
        >
          Sign out instead
        </button>
      }
    >
      <form onSubmit={handleSubmit} className="space-y-3.5">
        <Field label="New password" hint="At least 4 characters, and not the default.">
          <PasswordInput
            value={password}
            onChange={e => setPassword(e.target.value)}
            placeholder="••••••••"
            autoComplete="new-password"
            autoFocus
            required
          />
        </Field>

        <Field label="Confirm password">
          <PasswordInput
            value={confirm}
            onChange={e => setConfirm(e.target.value)}
            placeholder="••••••••"
            autoComplete="new-password"
            required
          />
        </Field>

        {error && <Banner tone="deny">{error}</Banner>}

        <Button
          type="submit"
          variant="primary"
          size="lg"
          loading={isSaving}
          className="w-full mt-1"
        >
          {isSaving ? 'Saving...' : 'Save and continue'}
        </Button>
      </form>
    </AuthShell>
  );
}
