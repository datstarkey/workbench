export type SplitDirection = 'horizontal' | 'vertical';

export interface ProjectTask {
	name: string;
	command: string;
}

export interface ProjectConfig {
	name: string;
	path: string;
	shell?: string;
	startupCommand?: string;
	tasks?: ProjectTask[];
}

export interface CreateTerminalRequest {
	id: string;
	projectPath: string;
	shell: string;
	cols: number;
	rows: number;
	startupCommand?: string;
}

export interface CreateTerminalResponse {
	id: string;
	backend: string;
}

export interface TerminalDataEvent {
	sessionId: string;
	data: string;
}

export interface TerminalExitEvent {
	sessionId: string;
	exitCode: number;
	signal?: number;
}

export interface ClaudeHookEvent {
	paneId: string;
	sessionId?: string;
	hookEventName?: string;
	source?: string;
	cwd?: string;
	transcriptPath?: string;
	hookPayload: Record<string, unknown>;
}

export interface CodexNotifyEvent {
	paneId: string;
	sessionId?: string;
	notifyEvent?: string;
	cwd?: string;
	codexPayload: Record<string, unknown>;
}

export type SessionType = 'shell' | 'claude' | 'codex';

export interface TerminalPaneState {
	id: string;
	startupCommand?: string;
	type?: SessionType;
	claudeSessionId?: string;
}

export interface TerminalTabState {
	id: string;
	label: string;
	split: SplitDirection;
	panes: TerminalPaneState[];
	type?: SessionType;
}

export interface ActiveClaudeSession {
	claudeSessionId: string;
	tabId: string;
	label: string;
	needsAttention?: boolean;
}

export interface DiscoveredClaudeSession {
	sessionId: string;
	label: string;
	timestamp: string;
	lastMessageRole?: 'user' | 'assistant';
}

export interface ProjectWorkspace {
	id: string;
	projectPath: string;
	projectName: string;
	terminalTabs: TerminalTabState[];
	activeTerminalTabId: string;
	worktreePath?: string;
	branch?: string;
}

// Git types

export interface GitInfo {
	branch: string;
	repoRoot: string;
	isWorktree: boolean;
}

export interface WorktreeInfo {
	path: string;
	head: string;
	branch: string;
	isMain: boolean;
}

export interface BranchInfo {
	name: string;
	sha: string;
	isCurrent: boolean;
	isRemote: boolean;
}

export interface WorktreeCopyOptions {
	aiConfig: boolean;
	envFiles: boolean;
}

export interface ProjectFormState {
	name: string;
	path: string;
	shell: string;
	startupCommand: string;
	tasks: ProjectTask[];
}
