/**
 * Control-plane transport abstraction.
 *
 * The same control-plane UI (project list, worktree creation, session spawn) can
 * run against two backends:
 *  - {@link TauriTransport} — the local desktop app, talking to the Rust backend
 *    over Tauri IPC.
 *  - {@link HttpTransport} — a remote `workbench-server`, talking over HTTP +
 *    WebSocket (used by the mobile app and by the desktop in "remote" mode).
 *
 * It deliberately covers the **control plane only**. Terminal IO
 * (`create/write/resize/kill_terminal`) is desktop-local and is NOT part of this
 * interface — remote clients spawn `claude remote-control` sessions instead and
 * continue them in the Claude mobile app. Use {@link Capabilities.terminalIO} to
 * feature-detect.
 */

import type {
	BranchInfo,
	ClaudeHookEvent,
	CodexNotifyEvent,
	DiscoveredClaudeSession,
	GitInfo,
	ProjectConfig,
	ProjectRefreshRequestedEvent,
	WorktreeInfo
} from '@workbench/types';

/** A spawned `claude remote-control` session (mirrors the server's RemoteSession). */
export interface RemoteSession {
	id: string;
	name: string | null;
	cwd: string;
	pid: number | null;
	status: 'starting' | 'running' | { exited: { code: number } };
	sessionUrl: string | null;
	startedAt: number;
}

/** A control-plane command name and its argument/result shapes. */
export interface ControlPlaneCommands {
	list_projects: { args: void; result: ProjectConfig[] };
	save_projects: { args: { projects: ProjectConfig[] }; result: void };
	load_workspaces: { args: void; result: unknown };
	save_workspaces: { args: { file: unknown }; result: void };
	list_worktrees: { args: { path: string }; result: WorktreeInfo[] };
	create_worktree: { args: { request: unknown }; result: string };
	remove_worktree: {
		args: { repoPath: string; worktreePath: string; force: boolean };
		result: void;
	};
	list_branches: { args: { path: string }; result: BranchInfo[] };
	git_info: { args: { path: string }; result: GitInfo };
	discover_claude_sessions: { args: { projectPath: string }; result: DiscoveredClaudeSession[] };
	discover_codex_sessions: { args: { projectPath: string }; result: DiscoveredClaudeSession[] };
	load_claude_settings: { args: { scope: string; projectPath?: string }; result: unknown };
	load_workbench_settings: { args: void; result: unknown };
	/** Spawn `claude remote-control` on the server (Claude only; Codex has none). */
	remote_spawn: {
		args: { projectPath: string; worktreePath?: string; name?: string };
		result: RemoteSession;
	};
	remote_sessions: { args: void; result: RemoteSession[] };
	remote_kill: { args: { id: string }; result: void };
}

/** Control-plane events streamed from the backend (NOT `terminal:data/exit`). */
export interface ControlPlaneEvents {
	'project:refresh-requested': ProjectRefreshRequestedEvent;
	'claude:hook': ClaudeHookEvent;
	'codex:notify': CodexNotifyEvent;
}

export type Unsubscribe = () => void;

export interface Capabilities {
	/** Local PTY terminals available (desktop) vs remote-only (mobile/remote). */
	terminalIO: boolean;
	/** Native OS dialogs available (folder picker, etc.). */
	nativeDialogs: boolean;
}

export interface ControlPlaneTransport {
	invoke<K extends keyof ControlPlaneCommands>(
		name: K,
		args: ControlPlaneCommands[K]['args']
	): Promise<ControlPlaneCommands[K]['result']>;

	subscribe<E extends keyof ControlPlaneEvents>(
		event: E,
		cb: (payload: ControlPlaneEvents[E]) => void
	): Promise<Unsubscribe>;

	readonly capabilities: Capabilities;
}
