import { useState, useEffect, FormEvent } from 'react';
import AuthShell from '@/components/AuthShell';
import { Banner, Button, Field, Input, PasswordInput } from '@/components/ui';

interface Props {
  auth: {
    login: (username: string, password: string) => Promise<boolean>;
    isLoading: boolean;
    error: string | null;
  };
}

export default function Login({ auth }: Props) {
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [notice, setNotice] = useState<string | null>(null);

  // Surface the reason a protected request bounced us here (e.g. an expired
  // session after a password change), stashed by the API layer before its hard
  // redirect. One-shot: clear it so it doesn't persist across later visits.
  useEffect(() => {
    const msg = sessionStorage.getItem('mcpgw_auth_message');
    if (msg) {
      setNotice(msg);
      sessionStorage.removeItem('mcpgw_auth_message');
    }
  }, []);

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    await auth.login(username, password);
  };

  return (
    <AuthShell title="MCP Gateway" description="Sign in to the control plane.">
      <form onSubmit={handleSubmit} className="space-y-3.5">
        <Field label="Username">
          <Input
            value={username}
            onChange={e => setUsername(e.target.value)}
            placeholder="admin"
            autoComplete="username"
            autoFocus
            required
            className="h-9"
          />
        </Field>

        <Field label="Password">
          <PasswordInput
            value={password}
            onChange={e => setPassword(e.target.value)}
            placeholder="••••••••"
            autoComplete="current-password"
            required
          />
        </Field>

        {notice && !auth.error && <Banner tone="neutral">{notice}</Banner>}
        {auth.error && <Banner tone="deny">{auth.error}</Banner>}

        <Button
          type="submit"
          variant="primary"
          size="lg"
          loading={auth.isLoading}
          className="w-full mt-1"
        >
          {auth.isLoading ? 'Signing in...' : 'Sign in'}
        </Button>
      </form>
    </AuthShell>
  );
}
