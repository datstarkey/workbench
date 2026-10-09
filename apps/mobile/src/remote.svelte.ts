import {
	createWorkspaceTransport,
	type CommandResult,
	type OpenEventSource,
	type Workspace,
	type WorkspaceCommand,
	type WorkspacePane,
	type WorkspaceServer,
	type WorkspaceTransport
} from './workspace-stream.ts';

/** How long a refresh waits for the new connection's snapshot. */
const REFRESH_WAIT_MS = 5000;

/**
 * The connected machine's workspaces and the one way to change them. The phone
 * only renders `workspaces` and sends commands; the host does the rest.
 */
export interface PaneRemote {
	readonly workspaces: Workspace[];
	/** Whether the host answered lately. */
	readonly online: boolean;
	/** Called after every change to `workspaces`. */
	onChange: (() => void) | null;
	/** Keep `workspaces` current (true while the app is in front). */
	follow(on: boolean): void;
	/** A refusal resolves with `error`; a host that can't be reached rejects. */
	command(cmd: WorkspaceCommand): Promise<CommandResult>;
	/** Catch up now (back from the lock screen, Refresh). */
	refresh(): Promise<void>;
	/** Whether a `restart` of this pane can do anything here. */
	canRestart(pane: WorkspacePane): boolean;
	dispose(): void;
}

/** A host with `workspaceApi`: the stream and the command route. */
export class WorkspaceRemote implements PaneRemote {
	workspaces = $state.raw<Workspace[]>([]);
	online = $state(true);
	onChange: (() => void) | null = null;
	private readonly transport: WorkspaceTransport;
	private unsubscribe: (() => void) | null = null;
	/** Refreshes waiting for the next snapshot. */
	private waiters: (() => void)[] = [];

	constructor(server: WorkspaceServer, openEventSource?: OpenEventSource) {
		this.transport = createWorkspaceTransport(server, openEventSource);
	}

	follow(on: boolean): void {
		if (!on) {
			this.unsubscribe?.();
			this.unsubscribe = null;
		} else if (!this.unsubscribe) {
			this.unsubscribe = this.transport.subscribeWorkspace(
				(s) => {
					this.workspaces = s.workspaces;
					for (const done of this.waiters.splice(0)) done();
					this.onChange?.();
				},
				(live) => (this.online = live)
			);
		}
	}

	command(cmd: WorkspaceCommand): Promise<CommandResult> {
		return this.transport.workspaceCommand(cmd);
	}

	/** A new connection starts with a full snapshot; resolves once it is here (or gives up). */
	async refresh(): Promise<void> {
		if (!this.unsubscribe) return;
		const next = new Promise<void>((resolve) => {
			this.waiters.push(resolve);
			setTimeout(resolve, REFRESH_WAIT_MS);
		});
		this.follow(false);
		this.follow(true);
		await next;
	}

	canRestart(pane: WorkspacePane): boolean {
		return pane.kind !== 'shell';
	}

	dispose(): void {
		this.follow(false);
		this.onChange = null;
	}
}
