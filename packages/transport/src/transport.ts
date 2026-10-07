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
 * It deliberately covers the **control plane only**. Terminal IO goes over the
 * server's terminal WebSocket and is NOT part of this interface. Use
 * {@link Capabilities.terminalIO} to feature-detect.
 */

import type {
	BranchInfo,
	ClaudeHookEvent,
	CodexNotifyEvent,
	DiscoveredClaudeSession,
	GitHubRemote,
	GitInfo,
	GitStatusResult,
	ProjectConfig,
	ProjectRefreshRequestedEvent,
	WorktreeInfo
} from '@workbench/types';

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
	git_status: { args: { path: string; projectPath?: string }; result: GitStatusResult };
	git_file_diff: {
		args: { path: string; projectPath?: string; file: string; staged: boolean };
		result: string;
	};
	/** `null` when the folder has no GitHub `origin`. */
	github_get_remote: { args: { path: string }; result: GitHubRemote | null };
	discover_claude_sessions: { args: { projectPath: string }; result: DiscoveredClaudeSession[] };
	discover_codex_sessions: { args: { projectPath: string }; result: DiscoveredClaudeSession[] };
	load_claude_settings: { args: { scope: string; projectPath?: string }; result: unknown };
	load_workbench_settings: { args: void; result: unknown };
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
