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
 *
 * This is deliberately written against raw tokens rather than the component
 * library: it has to render when something below it has already failed, and
 * pulling in more of the app to draw the failure screen is how a boundary ends
 * up throwing inside itself.
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
        <div className="min-h-screen flex items-center justify-center bg-void p-6">
          <div className="max-w-md w-full rounded-card border border-line bg-panel p-6 text-center shadow-[var(--shadow-card)]">
            <AlertTriangle className="mx-auto h-6 w-6 text-warn" />
            <h1 className="mt-3.5 text-md font-semibold text-ink">This page failed to render</h1>
            <p className="mt-2 text-xs text-ink-3 break-words">
              {this.state.error.message || 'No error details were reported.'}
            </p>
            <div className="mt-5 flex justify-center gap-2">
              <button
                onClick={this.handleReset}
                className="h-8 px-3 rounded-control border border-line bg-raised text-xs font-medium text-ink-2 hover:text-ink hover:border-line-strong transition-colors active:scale-[0.97]"
              >
                Try again
              </button>
              <button
                onClick={this.handleReload}
                className="h-8 px-3 rounded-control bg-solid text-on-solid text-xs font-semibold hover:bg-solid-hover transition-colors active:scale-[0.97]"
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
