import { Component, ErrorInfo, ReactNode } from 'react';
import { AlertTriangle } from 'lucide-react';

interface Props {
  children: ReactNode;
}

interface State {
  error: Error | null;
}

/**
 * Catches render-time exceptions anywhere below it so a single component throw
 * shows a recoverable error screen instead of white-screening the whole app.
 */
export default class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error('Uncaught render error:', error, info.componentStack);
  }

  handleReset = () => {
    this.setState({ error: null });
  };

  handleReload = () => {
    window.location.assign('/');
  };

  render() {
    if (this.state.error) {
      return (
        <div className="min-h-screen flex items-center justify-center bg-slate-950 p-6">
          <div className="max-w-md w-full rounded-xl border border-slate-800 bg-slate-900 p-6 text-center">
            <AlertTriangle className="mx-auto h-10 w-10 text-amber-400" />
            <h1 className="mt-4 text-lg font-semibold text-slate-100">Something went wrong</h1>
            <p className="mt-2 text-sm text-slate-400 break-words">
              {this.state.error.message || 'An unexpected error occurred.'}
            </p>
            <div className="mt-6 flex justify-center gap-3">
              <button
                onClick={this.handleReset}
                className="rounded-lg border border-slate-700 px-4 py-2 text-sm text-slate-200 hover:bg-slate-800"
              >
                Try again
              </button>
              <button
                onClick={this.handleReload}
                className="rounded-lg bg-indigo-600 px-4 py-2 text-sm text-white hover:bg-indigo-500"
              >
                Reload app
              </button>
            </div>
          </div>
        </div>
      );
    }

    return this.props.children;
  }
}
