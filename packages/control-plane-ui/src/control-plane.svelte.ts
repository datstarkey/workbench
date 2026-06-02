import type { ControlPlaneTransport, RemoteSession } from '@workbench/transport';
import type { ProjectConfig, WorktreeInfo } from '@workbench/types';

/**
 * Transport-driven control-plane state for the shared sidebar. Works against any
 * {@link ControlPlaneTransport} — the desktop app's local Tauri transport or a
 * remote `workbench-server` over HTTP. Covers the operations a remote client
 * needs: list projects, view/create worktrees, spawn `claude remote-control`
 * sessions, and manage running sessions. No terminal IO.
 */
export class ControlPlaneStore {
	private transport: ControlPlaneTransport;

	projects = $state<ProjectConfig[]>([]);
	sessions = $state<RemoteSession[]>([]);
	/** Worktrees per project path, loaded on demand. */
	worktrees = $state<Record<string, WorktreeInfo[]>>({});
	loading = $state(false);
	error = $state<string | null>(null);

	constructor(transport: ControlPlaneTransport) {
		this.transport = transport;
	}

	private async run<T>(fn: () => Promise<T>): Promise<T | undefined> {
		this.error = null;
		try {
			return await fn();
		} catch (e) {
			this.error = e instanceof Error ? e.message : String(e);
			return undefined;
		}
	}

	async refresh() {
		this.loading = true;
		await Promise.all([this.loadProjects(), this.refreshSessions()]);
		this.loading = false;
	}

	async loadProjects() {
		const projects = await this.run(() => this.transport.invoke('list_projects', undefined));
		if (projects) this.projects = projects;
	}

	async refreshSessions() {
		const sessions = await this.run(() => this.transport.invoke('remote_sessions', undefined));
		if (sessions) this.sessions = sessions;
	}

	async loadWorktrees(projectPath: string) {
		const list = await this.run(() =>
			this.transport.invoke('list_worktrees', { path: projectPath })
		);
		if (list) this.worktrees = { ...this.worktrees, [projectPath]: list };
	}

	async createWorktree(projectPath: string, branch: string) {
		const result = await this.run(() =>
			this.transport.invoke('create_worktree', {
				request: { repoPath: projectPath, branch, newBranch: true }
			})
		);
		if (result !== undefined) await this.loadWorktrees(projectPath);
		return result;
	}

	async spawn(projectPath: string, worktreePath?: string, name?: string) {
		const session = await this.run(() =>
			this.transport.invoke('remote_spawn', { projectPath, worktreePath, name })
		);
		if (session) {
			await this.refreshSessions();
			// The session URL/status update asynchronously once `claude` prints the
			// URL; poll a few times so the UI flips starting→running on its own.
			let tries = 0;
			const poll = setInterval(() => {
				void this.refreshSessions();
				if (++tries >= 5) clearInterval(poll);
			}, 2000);
		}
		return session;
	}

	async killSession(id: string) {
		await this.run(() => this.transport.invoke('remote_kill', { id }));
		await this.refreshSessions();
	}
}
