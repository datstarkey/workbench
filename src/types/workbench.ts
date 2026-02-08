export type SplitDirection = 'horizontal' | 'vertical';

export interface ProjectConfig {
	name: string;
	path: string;
	shell?: string;
	startupCommand?: string;
}

export interface ProjectsFile {
	projects: ProjectConfig[];
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

export interface TerminalWritePayload {
	sessionId: string;
	data: string;
}

export type SessionType = 'shell' | 'claude';

export type TerminalPaneState = {
	id: string;
	startupCommand?: string;
	type?: SessionType;
	claudeSessionId?: string;
};

export type TerminalTabState = {
	id: string;
	label: string;
	split: SplitDirection;
	panes: TerminalPaneState[];
	type?: SessionType;
};

export type ActiveClaudeSession = {
	claudeSessionId: string;
	tabId: string;
	label: string;
	needsAttention?: boolean;
};

export type DiscoveredClaudeSession = {
	sessionId: string;
	label: string;
	timestamp: string;
	lastMessageRole?: string;
};

export type ProjectWorkspace = {
	id: string;
	projectPath: string;
	projectName: string;
	terminalTabs: TerminalTabState[];
	activeTerminalTabId: string;
	worktreePath?: string;
	branch?: string;
};

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

export interface CreateWorktreeRequest {
	repoPath: string;
	branch: string;
	newBranch: boolean;
	path?: string;
}

export type ProjectFormState = {
	name: string;
	path: string;
	shell: string;
	startupCommand: string;
};
