export type SplitDirection = 'horizontal' | 'vertical';

/** Two terminal tabs of one workspace shown side by side (horizontal) or stacked (vertical). */
export interface SplitView {
	direction: SplitDirection;
	tabIds: [string, string];
}

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
	/**
	 * The chat view is the terminal's own `claude`, bridged by the Workbench
	 * plugin: switching views keeps the one process. Cleared on load (the
	 * terminal died with the app; the chat starts a new one, resumed).
	 */
	liveTerminal?: boolean;
	/** Claude account the pane's shell runs under (`CLAUDE_CONFIG_DIR`); absent is the default. */
	claudeAccountId?: string;
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
	/** Claude account whose config dir holds the transcript; absent is the default. */
	accountId?: string;
}

export interface ProjectWorkspace {
	id: string;
	projectPath: string;
	projectName: string;
	terminalTabs: TerminalTabState[];
	activeTerminalTabId: string;
	splitView?: SplitView;
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

/** Codex `approval_policy` override; `default` leaves Codex's own config in charge. */
export type CodexApprovalPolicy = 'default' | 'on-request' | 'never';

/** Codex `sandbox_mode` override; `default` leaves Codex's own config in charge. */
export type CodexSandboxMode = 'default' | 'read-only' | 'workspace-write' | 'danger-full-access';

export interface AgentAction {
	id: string;
	name: string;
	prompt: string;
	target: AgentActionTarget;
	category: string;
	tags: string[];
}

export interface WorkbenchSettings {
	/** How new Claude tabs open. */
	defaultClaudeView: PaneView;
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
	codexConfigApproved?: boolean | null;
	claudePermissionMode: ClaudePermissionMode;
	codexApprovalPolicy: CodexApprovalPolicy;
	codexSandboxMode: CodexSandboxMode;
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
	/** Extra Claude logins, each its own `CLAUDE_CONFIG_DIR`. `~/.claude` is implicit. */
	claudeAccounts?: ClaudeAccount[];
	/** Account new Claude sessions launch with; absent/null is the default `~/.claude`. */
	activeClaudeAccount?: string | null;
}

export interface ClaudeAccount {
	id: string;
	name: string;
	/** Absolute path, exported to the session as `CLAUDE_CONFIG_DIR`. */
	configDir: string;
}

/** `claude auth status --json`, trimmed to what the UI shows. */
export interface ClaudeAuthStatus {
	loggedIn: boolean;
	email?: string;
	orgName?: string;
	subscriptionType?: string;
}

/** One plan limit from `claude -p /usage`, e.g. label "session" or "week (all models)". */
export interface UsageLimit {
	label: string;
	percent: number;
	/** e.g. "Oct 2 at 9am (Europe/London)" */
	resets?: string;
	/** Unix seconds; Codex reports the reset as a time rather than text. */
	resetsAt?: number;
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

/** Which CLI a chat session runs. */
export type AgentKind = 'claude' | 'codex';

/**
 * Codex's approval + sandbox presets (its own `/approvals` picker):
 * `read-only` = read-only sandbox, asks on request; `auto` = workspace-write,
 * asks on request; `full-access` = no sandbox, never asks.
 */
export type CodexMode = 'read-only' | 'auto' | 'full-access';

/**
 * Chat items for a Claude or Codex chat session (mirror of workbench-core
 * `claude_transcript::TranscriptItem`), streamed by the server's
 * `/agent/{claude,codex}/:id/ws`.
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
			/** Names of the PDFs and text files attached (content isn't sent back). */
			files?: string[];
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
	| {
			/** An MCP server asks the person for input (MCP elicitation). `id` is the request id. */
			kind: 'elicitation';
			id: string;
			server: string;
			message: string;
			/** `form`: fill in `schema`; `url`: open `url` (sign-in, payment). */
			mode: 'form' | 'url';
			url?: string;
			/** The MCP `requestedSchema`: `{ type: 'object', properties, required? }` of primitive fields. */
			schema?: Record<string, unknown>;
			title?: string;
			description?: string;
			/** The agent withdrew the request (turn interrupted, timed out). */
			expired: boolean;
			/** URL mode: the server reported the flow finished. */
			completed: boolean;
			action?: ElicitationAction;
			/** What an accepted form sent. */
			content?: Record<string, unknown>;
	  }
	| { kind: 'notice'; id: string; text: string }
	/** Something around the conversation: a denied tool, a hook that failed or blocked, recalled memories, a refusal. */
	| {
			kind: 'event';
			id: string;
			event: TranscriptEventKind;
			title: string;
			detail?: string;
			/** Memory files (paths or URLs) for `memory`. */
			files?: string[];
	  };

export type TranscriptEventKind = 'permissionDenied' | 'hook' | 'memory' | 'refusal';

/** An artifact on claude.ai an `Artifact` call published or opened (mirror of core `ArtifactInfo`). */
export interface ArtifactInfo {
	toolUseId: string;
	url: string;
	title?: string;
	action: 'created' | 'updated' | 'opened' | 'published';
	version?: string;
}

export type ElicitationAction = 'accept' | 'decline' | 'cancel';

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
	/** Whose `<id>.output` file holds the live output, when not `id` itself. */
	outputId?: string;
}

/** A slash command the session accepts (built-ins, custom commands, skills). */
export interface SlashCommand {
	name: string;
	description: string;
	argumentHint?: string;
}

/** A model the session can switch to. */
export interface ModelOption {
	value: string;
	displayName: string;
	description: string;
	resolvedModel: string | null;
	/** Effort levels it accepts; empty when it has no effort setting. */
	effortLevels: EffortLevel[];
}

/** `none`/`minimal` are Codex-only, `max` Claude-only. */
export type EffortLevel = 'none' | 'minimal' | 'low' | 'medium' | 'high' | 'xhigh' | 'max';

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
	/** A `CodexMode` in a Codex chat; null there when Codex's own config is in charge. */
	permissionMode: PermissionMode | CodexMode | null;
	contextTokens: number | null;
	/** Claude only: unix ms when the prompt cache the latest API call used expires. */
	cacheExpiresAt?: number;
	/** Claude only: that cache's lifetime in seconds (3600 or 300). */
	cacheTtlSecs?: number;
	busy: boolean;
	tasks: TaskInfo[];
	retry: RetryInfo | null;
	rateLimit: RateLimitInfo | null;
	models: ModelOption[];
	/** The model picked in Workbench; null until one is. */
	modelChoice: string | null;
	/** The effort picked in Workbench; null means the model's default. */
	effort: EffortLevel | null;
	/** Codex only: the model's context window, in tokens (Claude's is inferred from the model). */
	contextWindow?: number;
	/** Codex only: plan limits as the stream reports them (Claude's come from `GET /agent/usage`). */
	usageLimits?: UsageLimit[];
	/** Claude only: artifacts this conversation's `Artifact` calls touched, in call order. */
	artifacts?: ArtifactInfo[];
	/** Claude only: a likely next prompt, until the next turn starts. */
	promptSuggestion?: string;
}

/** A chat's prompt cache upkeep, run by the server (Claude only). */
export interface CachePolicy {
	/** Unix ms; hidden keep-alive turns refresh the cache until then (at most 24h ahead). */
	keepWarmUntil?: number;
	/** Compact just before the cache expires (once keep-warm has ended). */
	compactOnExpiry: boolean;
}

export type AgentServerMsg =
	| {
			t: 'snapshot';
			sessionId: string;
			/** Index of the first item sent; older history stays on disk. */
			start: number;
			items: TranscriptItem[];
			meta: TranscriptMeta;
			commands: SlashCommand[];
			cachePolicy?: CachePolicy;
			exited: boolean;
	  }
	| { t: 'cachePolicy'; policy: CachePolicy }
	/** `[index, item]` pairs that were added or changed. */
	| { t: 'update'; changes: [number, TranscriptItem][]; meta: TranscriptMeta }
	/** `ended`: the person ended it (End session), not a crash, `/exit` or a handoff. */
	| { t: 'exit'; code: number | null; message: string | null; ended?: boolean }
	| { t: 'error'; message: string }
	/** The slash command list changed (sent apart from meta: it's large). */
	| { t: 'commands'; commands: SlashCommand[] }
	/** Reply to `output`: the whole output of a tool shown as a preview. */
	| { t: 'output'; toolId: string; text: string | null }
	/** Reply to `taskOutput`: the end of a background task's output, null until it exists. */
	| { t: 'taskOutput'; taskId: string; text: string | null; bytes: number | null }
	/** Reply to `rewind`: the file restore (or its preview, when `dryRun`), or why it failed. */
	| {
			t: 'rewind';
			messageId: string;
			dryRun: boolean;
			files: RewindFiles | null;
			error: string | null;
	  }
	/** The process restarted under the same id (a conversation rewind): attach again. */
	| { t: 'replaced' }
	| { t: 'revoked' };

/** Claude's answer to a file rewind (the Agent SDK's `RewindFilesResult`). */
export interface RewindFiles {
	canRewind: boolean;
	error?: string;
	/** Absolute paths; only on a dry run. */
	filesChanged?: string[];
	insertions?: number;
	deletions?: number;
}

/** An image attached to a chat message: base64 data the Claude API accepts. */
export interface ChatImage {
	mediaType: string;
	data: string;
	name: string;
}

/**
 * A PDF or text file attached to a chat message, sent to Claude as a document
 * block: `data` is base64 for a PDF and the text itself for a text file.
 */
export interface ChatFile {
	mediaType: 'application/pdf' | 'text/plain';
	data: string;
	name: string;
}

/** A file read for a chat message (desktop drag-and-drop, `read_chat_attachment`). */
export type ChatAttachment = ({ kind: 'image' } & ChatImage) | ({ kind: 'file' } & ChatFile);

export type AgentClientMsg =
	| { t: 'prompt'; text: string; images?: Omit<ChatImage, 'name'>[]; files?: ChatFile[] }
	| {
			t: 'approve';
			requestId: string;
			decision: ApprovalDecision;
			answers?: Record<string, string>;
	  }
	| {
			t: 'elicit';
			requestId: string;
			action: ElicitationAction;
			content?: Record<string, string | number | boolean | string[]>;
	  }
	| { t: 'interrupt' }
	| { t: 'mode'; mode: PermissionMode | CodexMode }
	| { t: 'model'; model: string }
	| { t: 'effort'; effort: EffortLevel }
	| { t: 'output'; toolId: string }
	| { t: 'taskOutput'; taskId: string }
	/** Refresh the prompt cache now with a hidden keep-alive turn. */
	| { t: 'cachePing' }
	| { t: 'cachePolicy'; policy: CachePolicy }
	/**
	 * Go back to before the prompt `messageId` (Claude only): restore the files
	 * changed since (`code`) and/or restart the conversation from there.
	 */
	| { t: 'rewind'; messageId: string; code: boolean; conversation: boolean; dryRun: boolean };

export interface StartAgentBody {
	/** Picks the route (`/agent/claude` or `/agent/codex`); absent is Claude. */
	agent?: AgentKind;
	projectPath: string;
	worktreePath?: string;
	/**
	 * Claude: required, a UUID the client picks. Codex: the thread to resume;
	 * absent starts a new thread, whose id the start call returns.
	 */
	sessionId?: string;
	permissionMode?: PermissionMode;
	/** Codex: the preset to start in; absent leaves `~/.codex/config.toml` in charge. */
	codexMode?: CodexMode;
	paneId?: string;
	hookSocket?: string;
	/** The pane's Claude account; absent is the default login. */
	claudeAccountId?: string;
	/** Join the running session only (another device's chat); 404 instead of spawning. */
	attachOnly?: boolean;
}

/** A running chat session, as `GET /agent` lists it (phone home screen). */
export interface AgentSummary {
	agent: AgentKind;
	sessionId: string;
	projectPath: string;
	worktreePath: string | null;
	paneId: string | null;
	/** The Claude account it runs under; null is the default login. */
	claudeAccountId: string | null;
	title: string | null;
	model: string | null;
	busy: boolean;
	exited: boolean;
	/** Unix ms: when the current turn started; null while idle. */
	busySince: number | null;
	/** Unix ms of the last change, for "12m ago". */
	updatedAt: number;
	/** Unix ms when the last turn went idle; null before one ends, absent from older servers. */
	turnEndedAt?: number | null;
	/** The oldest unanswered approval, question or MCP elicitation (`tool: 'Elicitation'`). */
	waiting: { id: string; tool: string; preview: string } | null;
	/** The newest tool call still running in this turn. */
	running: { name: string; detail: string } | null;
	/** Ids it ran under before a `/clear`, so a client holding one follows the re-key. */
	previousIds: string[];
	/** The server terminal whose interactive `claude` this chat is. */
	terminalId?: string;
}
