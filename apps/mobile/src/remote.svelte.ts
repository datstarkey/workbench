import {
	WorkspaceStream,
	workspaceCommand,
	type CommandResult,
	type OpenEventSource,
	type Workspace,
	type WorkspaceCommand,
	type WorkspaceServer
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
	/** Keep `workspaces` current (true while the app is in front). */
	follow(on: boolean): void;
	command(cmd: WorkspaceCommand): Promise<CommandResult>;
	/** Catch up now (back from the lock screen, Refresh). */
	refresh(): Promise<void>;
	dispose(): void;
}

/** A host with `workspaceApi`: the stream and the command route. */
export class WorkspaceRemote implements PaneRemote {
	workspaces = $state.raw<Workspace[]>([]);
	online = $state(true);
	private readonly server: WorkspaceServer;
	private readonly stream: WorkspaceStream;
	private following = false;
	/** Refreshes waiting for the next snapshot. */
	private waiters: (() => void)[] = [];

	constructor(server: WorkspaceServer, openEventSource?: OpenEventSource) {
		this.server = server;
		this.stream = new WorkspaceStream(
			{
				snapshot: (s) => {
					this.workspaces = s.workspaces;
					for (const done of this.waiters.splice(0)) done();
				},
				status: (live) => (this.online = live)
			},
			openEventSource
		);
	}

	follow(on: boolean): void {
		this.following = on;
		this.stream.follow(on ? this.server : null);
	}

	command(cmd: WorkspaceCommand): Promise<CommandResult> {
		return workspaceCommand(this.server, cmd);
	}

	/** A new connection starts with a full snapshot; resolves once it is here (or gives up). */
	async refresh(): Promise<void> {
		if (!this.following) return;
		const next = new Promise<void>((resolve) => {
			this.waiters.push(resolve);
			setTimeout(resolve, REFRESH_WAIT_MS);
		});
		this.stream.follow(null);
		this.stream.follow(this.server);
		await next;
	}

	dispose(): void {
		this.follow(false);
	}
}
