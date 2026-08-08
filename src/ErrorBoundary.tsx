import React from 'react';
import { invoke } from '@tauri-apps/api/core';

type State = { error?: Error };

export class ErrorBoundary extends React.Component<React.PropsWithChildren, State> {
  state: State = {};

  static getDerivedStateFromError(error: Error): State { return { error }; }

  componentDidCatch(error: Error, info: React.ErrorInfo) {
    invoke('report_error', {
      source: 'react',
      message: error.message,
      details: `${error.stack ?? ''}\n${info.componentStack ?? ''}`,
    }).catch(() => {});
  }

  render() {
    if (!this.state.error) return this.props.children;
    return (
      <main className="fatal-state">
        <div className="fatal-card">
          <strong>CypherLinks encountered an unexpected interface error.</strong>
          <p>A local diagnostic report was created. Restart the application; your download history and settings remain stored locally.</p>
          <button onClick={() => window.location.reload()}>Reload interface</button>
        </div>
      </main>
    );
  }
}
