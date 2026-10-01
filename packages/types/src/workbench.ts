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
	/** Legacy: no longer editable or run; kept so saving doesn't drop it from disk. */
	startupCommand?: string;
	/** Legacy: replaced by package.json scripts; kept so saving doesn't drop it from disk. */
	tasks?: ProjectTask[];
}

/** A runnable entry from a project's root `package.json` `scripts`. */
export interface PackageScript {
	name: string;
	command: string;
}

export type PackageManager = 'bun' | 'pnpm' | 'yarn' | 'npm';

/** A project's root `package.json`: its package manager and scripts (in file order). */
export interface PackageInfo {
	manager: PackageManager;
	managerVersion: string | null;
	/** `packageManager`, the lockfile name, or `default`. */
	detectedFrom: string;
	scripts: PackageScript[];
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
 * Desktop xterm path populates the optional desktop-parity fields (paneId,
 * hookSocket, shell) so the Claude/Codex hook bridge and the project shell work
 * identically to the local PtyManager path. ZDOTDIR shell-integration is applied
 * server-side (the resolver lives in workbench-core), so it is NOT a wire field.
 * Mobile omits the optional fields — server behaviour is unchanged for mobile.
 */
export interface CreateServerTerminalBody {
	projectPath: string;
	worktreePath?: string;
	name?: string;
	/** Optional command typed into the shell once it starts (e.g. `claude`). */
	command?: string;
	cols: number;
	rows: number;
	/** Opaque pane ID forwarded as WORKBENCH_PANE_ID env (desktop only). */
	paneId?: string;
	/** Hook-bridge address forwarded as WORKBENCH_HOOK_SOCKET env (desktop only). */
	hookSocket?: string;
	/** Project-configured shell to launch; empty/absent falls back to $SHELL. */
	shell?: string;
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
export type WsServerMsg = { t: 'takeover' } | { t: 'exit'; code: number | null };

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
	/** Claude panes can show their session as chat; the terminal keeps running underneath. */
	view?: PaneView;
}

export type PaneView = 'terminal' | 'chat';

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
	/** Not on any remote-tracking ref yet */
	unpushed: boolean;
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

/** Modes accepted by the Claude CLI's `--permission-mode` flag. */
export type ClaudePermissionMode =
	| 'default'
	| 'acceptEdits'
	| 'plan'
	| 'dontAsk'
	| 'auto'
	| 'bypassPermissions';

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
	claudePermissionMode: ClaudePermissionMode;
	/** Wrap Claude launches in @anthropic-ai/sandbox-runtime. */
	sandboxRuntimeEnabled: boolean;
	/** Domains the sandbox runtime permits egress to. */
	sandboxAllowedDomains: string[];
	cloneBaseDir?: string | null;
	accentColor?: AccentColor;
	serverMode?: boolean;
	serverPort?: number;
	/** Bearer token the LAN server requires; generated on first enable. */
	serverToken?: string | null;
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
}

/**
 * Chat items for a Claude chat session (mirror of workbench-core
 * `claude_transcript::TranscriptItem`), streamed by the server's
 * `/agent/claude/:id/ws`.
 */
export type TranscriptToolStatus = 'running' | 'ok' | 'error';

export interface TranscriptPatchHunk {
	oldStart: number;
	newStart: number;
	lines: string[];
}

export type ApprovalDecision = 'allow' | 'alwaysAllow' | 'deny';

export type PermissionMode =
	| 'default'
	| 'acceptEdits'
	| 'plan'
	| 'auto'
	| 'dontAsk'
	| 'bypassPermissions';

export type TranscriptItem =
	| {
			kind: 'user';
			id: string;
			text: string;
			timestamp: string;
			/** Images attached to the message (content isn't sent back). */
			images?: number;
	  }
	| { kind: 'text'; id: string; text: string }
	| { kind: 'thinking'; id: string; text: string }
	| {
			kind: 'tool';
			id: string;
			name: string;
			input: Record<string, unknown> | null;
			status: TranscriptToolStatus;
			output?: string;
			/** Size of the whole output when `output` is a preview (ask the server for it). */
			fullOutputBytes?: number;
			patch?: TranscriptPatchHunk[];
	  }
	| {
			kind: 'approval';
			id: string;
			tool: string;
			input: Record<string, unknown> | null;
			description?: string;
			blockedPath?: string;
			canAlwaysAllow: boolean;
			/** Claude withdrew the request (turn interrupted, answered elsewhere). */
			expired: boolean;
			decision?: ApprovalDecision;
			/** `AskUserQuestion` answers: question text → chosen label(s) or own words. */
			answers?: Record<string, string>;
	  }
	| { kind: 'notice'; id: string; text: string };

/** A subagent or background job Claude started (mirror of core `TaskInfo`). */
export interface TaskInfo {
	id: string;
	toolUseId?: string;
	/** `agent` for subagents; otherwise the CLI's task type, e.g. `local_bash`. */
	kind: string;
	subagentType?: string;
	description: string;
	status: 'pending' | 'running' | 'completed' | 'failed' | 'stopped' | 'killed' | 'paused';
	background: boolean;
	toolUses: number;
	tokens: number;
	durationMs: number;
	/** What it's doing right now. */
	activity?: string;
	lastTool?: string;
	summary?: string;
}

/** The API call is being retried (overloaded, rate limited, …). */
export interface RetryInfo {
	attempt: number;
	maxRetries: number;
	retryDelayMs: number;
	error: string | null;
}

/** Latest usage-limit status. */
export interface RateLimitInfo {
	status: 'allowed' | 'allowed_warning' | 'rejected';
	/** Unix seconds. */
	resetsAt: number | null;
	kind: string | null;
	/** 0–1 share used. */
	utilization: number | null;
}

export interface TranscriptMeta {
	title: string | null;
	model: string | null;
	permissionMode: PermissionMode | null;
	contextTokens: number | null;
	busy: boolean;
	tasks: TaskInfo[];
	retry: RetryInfo | null;
	rateLimit: RateLimitInfo | null;
}

export type AgentServerMsg =
	| {
			t: 'snapshot';
			sessionId: string;
			/** Index of the first item sent; older history stays on disk. */
			start: number;
			items: TranscriptItem[];
			meta: TranscriptMeta;
			exited: boolean;
	  }
	/** `[index, item]` pairs that were added or changed. */
	| { t: 'update'; changes: [number, TranscriptItem][]; meta: TranscriptMeta }
	| { t: 'exit'; code: number | null; message: string | null }
	| { t: 'error'; message: string }
	/** Reply to `output`: the whole output of a tool shown as a preview. */
	| { t: 'output'; toolId: string; text: string | null }
	/** Reply to `taskOutput`: the end of a background task's output, null until it exists. */
	| { t: 'taskOutput'; taskId: string; text: string | null; bytes: number | null }
	| { t: 'revoked' };

/** An image attached to a chat message: base64 data the Claude API accepts. */
export interface ChatImage {
	mediaType: string;
	data: string;
	name: string;
}

export type AgentClientMsg =
	| { t: 'prompt'; text: string; images?: Omit<ChatImage, 'name'>[] }
	| {
			t: 'approve';
			requestId: string;
			decision: ApprovalDecision;
			answers?: Record<string, string>;
	  }
	| { t: 'interrupt' }
	| { t: 'mode'; mode: PermissionMode }
	| { t: 'output'; toolId: string }
	| { t: 'taskOutput'; taskId: string };

export interface StartAgentBody {
	projectPath: string;
	worktreePath?: string;
	sessionId: string;
	permissionMode?: PermissionMode;
	paneId?: string;
	hookSocket?: string;
}
