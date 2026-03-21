/** Supported AI client applications — keep in sync with server's SUPPORTED_APPS */
export const SUPPORTED_APPS = ['claude', 'claudedesktop', 'cursor', 'vscode', 'openwebui', 'clawbot', 'codex', 'lmstudio'] as const;

export type AppSlug = (typeof SUPPORTED_APPS)[number];

export const APP_LABELS: Record<AppSlug, string> = {
  claude: 'Claude Code',
  claudedesktop: 'Claude Desktop',
  cursor: 'Cursor',
  vscode: 'VS Code',
  openwebui: 'Open WebUI',
  clawbot: 'Clawbot',
  codex: 'Codex',
  lmstudio: 'LM Studio',
};
