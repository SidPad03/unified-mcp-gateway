import { useState, FormEvent } from 'react';
import { Zap, Eye, EyeOff } from 'lucide-react';

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
  const [showPassword, setShowPassword] = useState(false);

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    await auth.login(username, password);
  };

  return (
    <div className="min-h-screen flex items-center justify-center bg-[#0a0a0f] p-4">
      <div className="w-full max-w-sm">
        {/* Logo */}
        <div className="text-center mb-8">
          <div className="w-14 h-14 bg-accent/15 rounded-2xl flex items-center justify-center mx-auto mb-4">
            <Zap className="w-7 h-7 text-accent" />
          </div>
          <h1 className="text-xl font-semibold text-white">MCP Gateway</h1>
          <p className="text-sm text-gray-500 mt-1">Sign in to your dashboard</p>
        </div>

        {/* Form */}
        <form onSubmit={handleSubmit} className="space-y-4">
          <div>
            <label className="block text-xs font-medium text-gray-400 mb-1.5 uppercase tracking-wider">Username</label>
            <input
              type="text"
              value={username}
              onChange={e => setUsername(e.target.value)}
              className="w-full px-3.5 py-2.5 bg-surface border border-border rounded-lg text-sm text-white placeholder-gray-600 focus:outline-none focus:border-accent/50 focus:ring-1 focus:ring-accent/20 transition-colors"
              placeholder="Enter username"
              required
            />
          </div>

          <div>
            <label className="block text-xs font-medium text-gray-400 mb-1.5 uppercase tracking-wider">Password</label>
            <div className="relative">
              <input
                type={showPassword ? 'text' : 'password'}
                value={password}
                onChange={e => setPassword(e.target.value)}
                className="w-full px-3.5 py-2.5 pr-10 bg-surface border border-border rounded-lg text-sm text-white placeholder-gray-600 focus:outline-none focus:border-accent/50 focus:ring-1 focus:ring-accent/20 transition-colors"
                placeholder="Enter password"
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

          {auth.error && (
            <div className="px-3 py-2 bg-danger/10 border border-danger/20 rounded-lg">
              <p className="text-xs text-danger">{auth.error}</p>
            </div>
          )}

          <button
            type="submit"
            disabled={auth.isLoading}
            className="w-full py-2.5 bg-accent hover:bg-accent-hover text-white text-sm font-medium rounded-lg transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
          >
            {auth.isLoading ? 'Signing in...' : 'Sign in'}
          </button>

          <p className="text-center text-xs text-gray-600">
            Sign in with your credentials
          </p>
        </form>
      </div>
    </div>
  );
}
