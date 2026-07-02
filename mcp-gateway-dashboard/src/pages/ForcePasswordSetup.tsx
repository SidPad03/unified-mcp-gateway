import { useState, FormEvent } from 'react';
import { ShieldCheck, Eye, EyeOff } from 'lucide-react';
import { api, User } from '@/lib/api';

interface Props {
  user: User;
  onComplete: () => void;
  onLogout: () => void;
}

export default function ForcePasswordSetup({ user, onComplete, onLogout }: Props) {
  const [password, setPassword] = useState('');
  const [confirm, setConfirm] = useState('');
  const [showPassword, setShowPassword] = useState(false);
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
      setError('Please choose a password other than the default');
      return;
    }
    if (password !== confirm) {
      setError('Passwords do not match');
      return;
    }

    setIsSaving(true);
    try {
      await api.updateUser(user.user_id, { password });
      onComplete();
    } catch (err: any) {
      setError(err.message || 'Failed to set password');
    } finally {
      setIsSaving(false);
    }
  };

  return (
    <div className="min-h-screen flex items-center justify-center bg-[#0a0a0f] p-4">
      <div className="w-full max-w-sm">
        {/* Header */}
        <div className="text-center mb-8">
          <div className="w-14 h-14 bg-accent/15 rounded-2xl flex items-center justify-center mx-auto mb-4">
            <ShieldCheck className="w-7 h-7 text-accent" />
          </div>
          <h1 className="text-xl font-semibold text-white">Set your password</h1>
          <p className="text-sm text-gray-500 mt-1">
            Choose a new password for <span className="text-gray-300">{user.username}</span> before continuing
          </p>
        </div>

        {/* Form */}
        <form onSubmit={handleSubmit} className="space-y-4">
          <div>
            <label className="block text-xs font-medium text-gray-400 mb-1.5 uppercase tracking-wider">New password</label>
            <div className="relative">
              <input
                type={showPassword ? 'text' : 'password'}
                value={password}
                onChange={e => setPassword(e.target.value)}
                className="w-full px-3.5 py-2.5 pr-10 bg-surface border border-border rounded-lg text-sm text-white placeholder-gray-600 focus:outline-none focus:border-accent/50 focus:ring-1 focus:ring-accent/20 transition-colors"
                placeholder="Enter new password"
                autoFocus
                required
              />
              <button
                type="button"
                onClick={() => setShowPassword(!showPassword)}
                className="absolute right-3 top-1/2 -translate-y-1/2 text-gray-500 hover:text-gray-300"
              >
                {showPassword ? <EyeOff className="w-4 h-4" /> : <Eye className="w-4 h-4" />}
              </button>
            </div>
          </div>

          <div>
            <label className="block text-xs font-medium text-gray-400 mb-1.5 uppercase tracking-wider">Confirm password</label>
            <input
              type={showPassword ? 'text' : 'password'}
              value={confirm}
              onChange={e => setConfirm(e.target.value)}
              className="w-full px-3.5 py-2.5 bg-surface border border-border rounded-lg text-sm text-white placeholder-gray-600 focus:outline-none focus:border-accent/50 focus:ring-1 focus:ring-accent/20 transition-colors"
              placeholder="Re-enter new password"
              required
            />
          </div>

          {error && (
            <div className="px-3 py-2 bg-danger/10 border border-danger/20 rounded-lg">
              <p className="text-xs text-danger">{error}</p>
            </div>
          )}

          <button
            type="submit"
            disabled={isSaving}
            className="w-full py-2.5 bg-accent hover:bg-accent-hover text-white text-sm font-medium rounded-lg transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
          >
            {isSaving ? 'Saving...' : 'Save password & continue'}
          </button>

          <button
            type="button"
            onClick={onLogout}
            className="w-full text-center text-xs text-gray-600 hover:text-gray-400 transition-colors"
          >
            Sign out
          </button>
        </form>
      </div>
    </div>
  );
}
