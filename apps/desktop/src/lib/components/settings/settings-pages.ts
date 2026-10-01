import type { SettingsScope } from '$types/claude-settings';

/**
 * Which store a page edits, and so what Save/Reset act on. `immediate` pages
 * apply each change as it is made and have no Save button.
 */
export type SettingsStoreKind = 'workbench' | 'claude' | 'immediate';

export type SettingsPageId =
	| 'general'
	| 'worktrees'
	| 'terminal'
	| 'agent-actions'
	| 'integrations'
	| 'remote-access'
	| 'claude-sessions'
	| 'behaviour'
	| 'permissions'
	| 'bash-sandbox'
	| 'mcp'
	| 'plugins'
	| 'hooks'
	| 'skills'
	| 'codex-sessions';

export interface SettingsPage {
	id: SettingsPageId;
	/** Nav label. */
	label: string;
	/** Page heading, when the nav label is ambiguous on its own. */
	title?: string;
	description: string;
	store: SettingsStoreKind;
}

export interface SettingsNavGroup {
	label: string;
	/** A subheading inside the group above, rather than a group of its own. */
	sub?: boolean;
	pages: SettingsPage[];
}

export const SETTINGS_NAV: SettingsNavGroup[] = [
	{
		label: 'Workbench',
		pages: [
			{
				id: 'general',
				label: 'General',
				description: 'Look and layout of Workbench itself.',
				store: 'workbench'
			},
			{
				id: 'worktrees',
				label: 'Worktrees',
				description: 'Where new worktrees go and what they branch from.',
				store: 'workbench'
			},
			{
				id: 'terminal',
				label: 'Terminal',
				description: 'Rendering and performance of terminal panes.',
				store: 'workbench'
			},
			{
				id: 'agent-actions',
				label: 'Agent actions',
				description:
					'Reusable prompts. Each opens a new Claude or Codex session and submits the prompt straight away.',
				store: 'workbench'
			},
			{
				id: 'integrations',
				label: 'Integrations',
				description: 'Outside services Workbench talks to.',
				store: 'workbench'
			},
			{
				id: 'remote-access',
				label: 'Remote access',
				description:
					'Let your phone or another computer create worktrees and open Claude sessions on this machine.',
				store: 'immediate'
			}
		]
	},
	{
		label: 'Claude Code',
		pages: [
			{
				id: 'claude-sessions',
				label: 'Sessions',
				title: 'Claude sessions',
				description: 'How Workbench launches Claude. Applies to every account and project.',
				store: 'workbench'
			}
		]
	},
	{
		label: 'settings.json',
		sub: true,
		pages: [
			{
				id: 'behaviour',
				label: 'Behaviour',
				description: 'Claude Code’s own preferences.',
				store: 'claude'
			},
			{
				id: 'permissions',
				label: 'Permissions',
				description: 'Which tools Claude may use without asking.',
				store: 'claude'
			},
			{
				id: 'bash-sandbox',
				label: 'Bash sandbox',
				description:
					'Claude Code’s own sandbox, for Bash commands only. To confine the whole session, use Sessions › Sandbox runtime.',
				store: 'claude'
			},
			{
				id: 'mcp',
				label: 'MCP servers',
				description: 'MCP servers defined in this settings file.',
				store: 'claude'
			},
			{
				id: 'plugins',
				label: 'Plugins',
				description: 'Plugins installed in ~/.claude/plugins/.',
				store: 'claude'
			},
			{
				id: 'hooks',
				label: 'Hooks',
				description: 'Commands Claude Code runs on session events.',
				store: 'claude'
			},
			{
				id: 'skills',
				label: 'Skills',
				description: 'Skills found in ~/.claude/skills/. Each is a folder with a SKILL.md.',
				store: 'claude'
			}
		]
	},
	{
		label: 'Codex',
		pages: [
			{
				id: 'codex-sessions',
				label: 'Sessions',
				title: 'Codex sessions',
				description:
					'How Workbench launches Codex. Codex default keeps whatever your config.toml says.',
				store: 'workbench'
			}
		]
	}
];

const PAGES = new Map(SETTINGS_NAV.flatMap((g) => g.pages).map((p) => [p.id, p]));

export function settingsPage(id: SettingsPageId): SettingsPage {
	return PAGES.get(id)!;
}

export const WORKBENCH_SETTINGS_FILE = '~/.workbench/settings.json';

/** The file a Claude settings scope saves to, for the footer. */
export function claudeSettingsFile(scope: SettingsScope, projectName: string): string {
	const dir = scope.startsWith('project') ? `${projectName}/.claude` : '~/.claude';
	return `${dir}/${scope.endsWith('local') ? 'settings.local.json' : 'settings.json'}`;
}
