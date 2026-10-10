/**
 * The server-owned workspace model (`workbench_core::workspace`) as clients
 * see it: `GET /events/workspace` snapshots and `POST /workspace/commands`.
 * See docs/WORKSPACE_MODEL.md.
 */
import type { AgentSummary, SplitDirection, SplitView, TerminalRenderer } from './workbench.ts';

export type PaneKind = 'shell' | 'claude' | 'codex';
/** Which process a Codex pane runs: the TUI in a terminal, or `codex app-server` (chat). */
export type CodexPaneMode = 'tui' | 'appServer';
export type PaneStatus = 'starting' | 'needsTrust' | 'running' | 'exited';

/** A pane with its process's runtime state merged in (never saved). */
export interface WorkspacePane {
	id: string;
	kind: PaneKind;
	sessionId?: string;
	previousIds?: string[];
	accountId?: string;
	prompt?: string;
	command?: string;
	codexMode?: CodexPaneMode;
	terminalId: string | null;
	status: PaneStatus;
	title: string | null;
	busy: boolean;
	/** Unix ms: the current turn's start, the last turn's end. */
	busySince?: number | null;
	turnEndedAt?: number | null;
	/** The tool call the turn is running. */
	running?: AgentSummary['running'];
	/** Subagents and background tasks still going; absent from older hosts. */
	runningTasks?: AgentSummary['runningTasks'];
	waiting: AgentSummary['waiting'];
	/** Unix ms the pane started waiting on `waiting`. */
	waitingSince?: number | null;
	/** Why the last spawn failed. */
	error: string | null;
	/** Something to tell the person about how this spawn started; show it once. */
	notice?: string | null;
	/** Bumped by every spawn (start, restart, mode switch). */
	generation: number;
}

export interface WorkspaceTab {
	id: string;
	label: string;
	kind: PaneKind;
	split: SplitDirection;
	panes: WorkspacePane[];
}

export interface ServerWorkspace {
	id: string;
	projectPath: string;
	projectName: string;
	worktreePath?: string;
	branch?: string;
	renderer: TerminalRenderer;
	tabs: WorkspaceTab[];
	splitView?: SplitView;
	/** Opened only to host a session started by path; closes with its last tab. */
	transient?: boolean;
}

/** Whether the server saves the model: `locked` (another process keeps it) and `error` (unreadable) don't. */
export interface WorkspacePersistence {
	status: 'ok' | 'locked' | 'error';
	message: string | null;
}

/** The desktop's per-device state saved with an older model, for it to take over once. */
export interface WorkspaceLocalSeed {
	selectedId?: string | null;
	activeTabIds?: Record<string, string>;
	chatPanes?: string[];
}

export interface WorkspaceSnapshot {
	rev: number;
	workspaces: ServerWorkspace[];
	/** Absent from servers that predate it. */
	persistence?: WorkspacePersistence;
	local?: WorkspaceLocalSeed;
}

/** Where a new session goes: a workspace, or a project (and worktree) by path. */
export type WorkspaceTarget =
	| { workspaceId: string }
	| { projectPath: string; worktreePath?: string; projectName?: string; branch?: string };

export type WorkspaceCommand =
	| {
			type: 'openWorkspace';
			projectPath: string;
			projectName: string;
			worktreePath?: string;
			branch?: string;
			renderer?: TerminalRenderer;
	  }
	| { type: 'closeWorkspace'; workspaceId: string }
	| { type: 'closeProject'; projectPath: string }
	| ({
			type: 'newSession';
			kind: PaneKind;
			/** Makes a retry of this command apply once (the transport sets it). */
			requestId?: string;
			resume?: string;
			prompt?: string;
			accountId?: string;
			label?: string;
			command?: string;
			codexMode?: CodexPaneMode;
	  } & WorkspaceTarget)
	| { type: 'closePane'; paneId: string }
	| { type: 'closeTab'; tabId: string }
	| { type: 'restart'; tabId: string }
	| { type: 'setCodexMode'; paneId: string; mode: CodexPaneMode }
	| { type: 'rename'; tabId: string; label: string }
	| { type: 'split'; tabId: string; direction: SplitDirection }
	| { type: 'movePane'; paneId: string; tabId: string }
	| { type: 'moveTab'; tabId: string; toTabId: string }
	| { type: 'moveWorkspace'; workspaceId: string; toWorkspaceId: string }
	| { type: 'trustFolder'; paneId: string }
	| { type: 'updateProject'; projectPath: string; newPath: string; projectName: string };

/** `POST /workspace/commands`: what the command opened or found, at the `rev` that shows it. */
export interface WorkspaceCommandResult {
	rev: number;
	workspaceId?: string | null;
	tabId?: string | null;
	paneId?: string | null;
}

/** Each pane's spawn notice, keyed by pane and spawn so a host shows each one once. */
export function paneNotices(workspaces: ServerWorkspace[]): { key: string; notice: string }[] {
	return workspaces.flatMap((w) =>
		w.tabs.flatMap((t) =>
			t.panes.flatMap((p) =>
				p.notice ? [{ key: `${p.id}:${p.generation}`, notice: p.notice }] : []
			)
		)
	);
}
