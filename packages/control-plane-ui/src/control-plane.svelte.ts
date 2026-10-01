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
	/** GitHub web URL per project path (null when it has none), loaded on demand. */
	githubUrls = $state<Record<string, string | null>>({});
	loading = $state(false);
	error = $state<string | null>(null);

	/** Active spawn-status poll intervals, so they can be cancelled on dispose. */
	// eslint-disable-next-line svelte/prefer-svelte-reactivity -- internal bookkeeping only
	private pollTimers = new Set<ReturnType<typeof setInterval>>();
	// eslint-disable-next-line svelte/prefer-svelte-reactivity -- internal bookkeeping only
	private githubUrlsInFlight = new Set<string>();

	constructor(transport: ControlPlaneTransport) {
		this.transport = transport;
	}

	/** Stop all background polling. Call when the store is no longer used (e.g. the
	 *  remote instance owning it is removed) so timers don't keep hitting the server. */
	dispose() {
		for (const t of this.pollTimers) clearInterval(t);
		this.pollTimers.clear();
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
		await Promise.all([
			this.loadProjects(),
			this.refreshSessions(),
			...Object.keys(this.githubUrls).map((path) => this.fetchGithubUrl(path))
		]);
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

	/** Loads once per path; `refresh()` re-fetches the loaded ones. */
	async loadGithubUrl(projectPath: string) {
		if (!(projectPath in this.githubUrls)) await this.fetchGithubUrl(projectPath);
	}

	/** Best-effort, so it never sets `error`: a failed request keeps what was cached
	 *  (nothing, the first time) and a later call retries. */
	private async fetchGithubUrl(projectPath: string) {
		if (this.githubUrlsInFlight.has(projectPath)) return;
		this.githubUrlsInFlight.add(projectPath);
		try {
			const remote = await this.transport.invoke('github_get_remote', { path: projectPath });
			this.githubUrls = { ...this.githubUrls, [projectPath]: remote?.htmlUrl ?? null };
		} catch {
			// Unreachable server: keep the cached value.
		} finally {
			this.githubUrlsInFlight.delete(projectPath);
		}
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
				if (++tries >= 5) {
					clearInterval(poll);
					this.pollTimers.delete(poll);
				}
			}, 2000);
			this.pollTimers.add(poll);
		}
		return session;
	}

	async killSession(id: string) {
		await this.run(() => this.transport.invoke('remote_kill', { id }));
		await this.refreshSessions();
	}
}
