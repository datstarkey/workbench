export type SplitDirection = 'horizontal' | 'vertical';

export interface ProjectTask {
	name: string;
	command: string;
}

export interface ProjectConfig {
	name: string;
	path: string;
	group?: string;
	shell?: string;
	startupCommand?: string;
	tasks?: ProjectTask[];
}

// ── Native (SwiftTerm / PtyManager) terminal types ──────────────────────────
// These are used ONLY by the native SwiftTerm path (pty.rs / native_terminal.rs)
// and the corresponding Tauri IPC commands (create_terminal / terminal:data /
// terminal:exit). The xterm path no longer uses them — xterm attaches over
// WebSocket to TerminalManager in the embedded server.

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

// ── Server terminal types (xterm over WebSocket) ─────────────────────────────
// Used by the desktop xterm path (ws://127.0.0.1:<port>/remote/terminals/:id/ws)
// and the mobile client. Mirror of the Rust structs in apps/server/src/terminal.rs.

/**
 * Request body for POST /remote/terminals.
 *
 * Desktop xterm path populates the optional desktop-parity env fields
 * (paneId, hookSocket, zdotdir, origZdotdir) so that shell integration and
 * the Claude hook bridge work identically to the local PtyManager path.
 * Mobile omits them — server behaviour is unchanged for mobile.
 */
export interface CreateServerTerminalBody {
	projectPath: string;
	worktreePath?: string;
	name?: string;
	/** Optional command typed into the shell once it starts (e.g. `claude`). */
	command?: string;
	cols: number;
	rows: number;
	/** Opaque pane ID forwarded as CLAUDE_PANE_ID env (desktop only). */
	paneId?: string;
	/** Unix socket path for the Claude hook bridge — CLAUDE_HOOK_SOCKET env (desktop only). */
	hookSocket?: string;
	/** Custom ZDOTDIR for zsh shell integration (desktop only). */
	zdotdir?: string;
	/** Original ZDOTDIR restored inside the injected .zshrc (desktop only). */
	origZdotdir?: string;
}

/**
 * Metadata returned by POST /remote/terminals and GET /remote/terminals.
 * Mirrors `TerminalMeta` in apps/server/src/terminal.rs.
 */
export interface ServerTerminalMeta {
	id: string;
	name?: string;
	cwd: string;
	/** Unix epoch milliseconds. */
	createdAt: number;
	alive: boolean;
}

/**
 * Server → client control messages sent as JSON text frames over the terminal
 * WebSocket. PTY output is still delivered as binary frames.
 *
 * Discriminate by frame type:
 * - `MessageEvent.data` is an `ArrayBuffer` → raw PTY bytes (write to xterm)
 * - `MessageEvent.data` is a `string`        → parse as `WsServerMsg` (control)
 */
export type WsServerMsg =
	| { t: 'takeover' }
	| { t: 'exit'; code: number | null };

export interface TerminalActivityEvent {
	sessionId: string;
	active: boolean;
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
export type AISessionType = 'claude' | 'codex';

/** Type guard: true for 'claude' and 'codex' session types */
export function isAISessionType(type: SessionType | undefined): type is AISessionType {
	return type === 'claude' || type === 'codex';
}

export interface TerminalPaneState {
	id: string;
	startupCommand?: string;
	type?: SessionType;
	claudeSessionId?: string;
	/**
	 * When set, this pane's xterm is backed by a server TerminalManager session
	 * (WS path). Persisted so the pane can reattach to the same PTY after a
	 * webview reload. Absent for panes that have not yet been created or that
	 * use the native SwiftTerm path.
	 */
	serverTerminalId?: string;
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
	sessionType: 'claude' | 'codex';
	needsAttention?: boolean;
	awaitingInput?: boolean;
	worktreePath?: string;
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
	renderer?: TerminalRenderer;
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

export interface GitFileStatus {
	path: string;
	status: string;
	staged: boolean;
	unstaged: boolean;
}

export interface GitStatusResult {
	branch: string;
	files: GitFileStatus[];
	ahead: number;
	behind: number;
	hasUpstream: boolean;
}

export interface GitLogEntry {
	sha: string;
	shortSha: string;
	message: string;
	author: string;
	date: string;
}

export interface GitStashEntry {
	index: number;
	message: string;
	date: string;
}

export interface GitCommitResult {
	sha: string;
	message: string;
}

export interface GitCommitFile {
	path: string;
	status: string;
}

export interface WorktreeCopyOptions {
	aiConfig: boolean;
	envFiles: boolean;
}

export interface GitChangedEvent {
	projectPath: string;
}

export interface ProjectRefreshRequestedEvent {
	projectPath: string;
	source: string;
	trigger: string;
}

export interface GitHubProjectStatusEvent {
	projectPath: string;
	status: GitHubProjectStatus;
}

export interface GitHubCheckTransitionEvent {
	projectPath: string;
	prNumber: number;
	name: string;
	bucket: 'pass' | 'fail';
}

export interface TrelloMergeActionAppliedEvent {
	projectPath: string;
	branch: string;
	cardId: string;
}

// GitHub types

export interface GitHubRepo {
	name: string;
	nameWithOwner: string;
	description?: string | null;
	isPrivate: boolean;
	isFork: boolean;
	/** Web URL, e.g. https://github.com/owner/repo — also usable as HTTP clone URL */
	url: string;
	sshUrl: string;
}

export interface GitHubRemote {
	owner: string;
	repo: string;
	htmlUrl: string;
}

export interface GitHubChecksStatus {
	overall: 'success' | 'failure' | 'pending' | 'none';
	total: number;
	passing: number;
	failing: number;
	pending: number;
}

export interface GitHubPR {
	number: number;
	title: string;
	state: 'OPEN' | 'CLOSED' | 'MERGED';
	url: string;
	isDraft: boolean;
	headRefName: string;
	reviewDecision: 'APPROVED' | 'CHANGES_REQUESTED' | 'REVIEW_REQUIRED' | null;
	checksStatus: GitHubChecksStatus;
	mergeStateStatus:
		| 'BEHIND'
		| 'BLOCKED'
		| 'CLEAN'
		| 'DIRTY'
		| 'DRAFT'
		| 'HAS_HOOKS'
		| 'UNKNOWN'
		| 'UNSTABLE'
		| null;
	actions: GitHubPRActions;
}

export interface GitHubPRActions {
	canMerge: boolean;
	canMarkReady: boolean;
	canUpdateBranch: boolean;
}

export interface MergePrOptions {
	method: 'squash' | 'merge' | 'rebase';
	deleteBranch: boolean;
	admin: boolean;
	auto: boolean;
}

export interface GitHubProjectStatus {
	remote: GitHubRemote | null;
	prs: GitHubPR[];
	branchRuns: Record<string, GitHubBranchRuns>;
	prChecks: Record<number, GitHubCheckDetail[]>;
}

export interface GitHubCheckDetail {
	name: string;
	bucket: 'pass' | 'fail' | 'pending' | 'skipping' | 'cancel';
	workflow: string;
	link: string;
	startedAt: string | null;
	completedAt: string | null;
	description: string;
}

export interface GitHubWorkflowRun {
	id: number;
	name: string;
	displayTitle: string;
	headBranch: string;
	status: 'queued' | 'in_progress' | 'completed';
	conclusion: 'success' | 'failure' | 'cancelled' | 'skipped' | null;
	url: string;
	event: string;
	createdAt: string;
	updatedAt: string;
}

export interface GitHubBranchRuns {
	status: GitHubChecksStatus;
	runs: GitHubWorkflowRun[];
}

export interface GitHubBranchStatus {
	pr: GitHubPR | null;
	remote: GitHubRemote | null;
	branchRuns: GitHubBranchRuns | null;
}

// Workbench app settings

export type WorktreeStrategy = 'sibling' | 'inside';
export type WorktreeStartPoint = 'auto' | 'current' | 'custom';
export type TerminalPerformanceMode = 'auto' | 'always';
export type TerminalRenderer = 'xterm' | 'native';
export type AccentColor = 'violet' | 'tideline' | 'ember' | 'moss' | 'iris';

export type AgentActionTarget = 'claude' | 'codex' | 'both';

export interface AgentAction {
	id: string;
	name: string;
	prompt: string;
	target: AgentActionTarget;
	category: string;
	tags: string[];
}

export interface WorkbenchSettings {
	worktreeStrategy: WorktreeStrategy;
	worktreeFetchBeforeCreate: boolean;
	worktreeStartPoint: WorktreeStartPoint;
	worktreeCustomBranch: string;
	trelloEnabled: boolean;
	gitSidebarEnabled: boolean;
	terminalPerformanceMode: TerminalPerformanceMode;
	terminalTelemetryEnabled: boolean;
	terminalRenderer: TerminalRenderer;
	agentActions: AgentAction[];
	claudeHooksApproved?: boolean | null;
	codexConfigApproved?: boolean | null;
	useHappyCoder: boolean;
	cloneBaseDir?: string | null;
	accentColor?: AccentColor;
	serverMode?: boolean;
	serverPort?: number;
	settingsWindowBounds?: SettingsWindowBounds | null;
}

/** Persisted position + size of the draggable settings window. */
export interface SettingsWindowBounds {
	x: number;
	y: number;
	width: number;
	height: number;
}

export interface IntegrationStatus {
	needsChanges: boolean;
	description: string;
}

export interface HookLogEntry {
	timestamp: string;
	level: 'event' | 'error';
	eventName?: string;
	paneId?: string;
	source?: string;
	summary: string;
	toolName?: string;
}

export interface ProjectFormState {
	name: string;
	path: string;
	group: string;
	shell: string;
	startupCommand: string;
	tasks: ProjectTask[];
}
