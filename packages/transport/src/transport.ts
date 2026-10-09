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
	CodexNotifyEvent,
	DiscoveredClaudeSession,
	GitHubRemote,
	GitInfo,
	GitStatusResult,
	HostUpdateStarted,
	HostUpdateStatus,
	ProjectConfig,
	ProjectRefreshRequestedEvent,
	WorkspaceCommand,
	WorkspaceCommandResult,
	WorktreeInfo
} from '@workbench/types';
import type { WorkspaceStreamHandlers } from './workspace.ts';

/** A control-plane command name and its argument/result shapes. */
export interface ControlPlaneCommands {
	list_projects: { args: void; result: ProjectConfig[] };
	save_projects: { args: { projects: ProjectConfig[] }; result: void };
	list_worktrees: { args: { path: string }; result: WorktreeInfo[] };
	create_worktree: { args: { request: unknown }; result: string };
	remove_worktree: {
		args: { repoPath: string; worktreePath: string; force: boolean; deleteBranch: boolean };
		/** Absent from hosts that predate `deleteBranch`. */
		result: { branchDeleted?: boolean } | null;
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
	/** The host's account for new Claude sessions (`null`: the default login); changes only that setting. */
	set_active_claude_account: { args: { id: string | null }; result: void };
	/** Server only: the app hosting it (the desktop). 501 from a standalone server. */
	host_update_status: { args: { fresh?: boolean } | void; result: HostUpdateStatus };
	/** Server only: install the reviewed `version`; the host restarts. 501 from a standalone server. */
	host_update_install: { args: { version?: string } | void; result: HostUpdateStarted };
}

/** Control-plane events streamed from the backend (NOT `terminal:data/exit`). */
export interface ControlPlaneEvents {
	'project:refresh-requested': ProjectRefreshRequestedEvent;
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

	/** One command to the server's workspace model (`POST /workspace/commands`). */
	workspaceCommand(cmd: WorkspaceCommand): Promise<WorkspaceCommandResult>;

	/** Follow the workspace model's snapshots (`GET /events/workspace`), reconnecting on failure. */
	subscribeWorkspace(handlers: WorkspaceStreamHandlers): Unsubscribe;

	readonly capabilities: Capabilities;
}
