import type { ControlPlaneTransport, Unsubscribe } from '@workbench/transport';
import type { ServerWorkspace, WorkspaceCommand, WorkspaceCommandResult } from '@workbench/types';

/** How long a refresh waits for the new connection's snapshot. */
const REFRESH_WAIT_MS = 5000;

/**
 * The connected machine's workspaces and the one way to change them. The phone
 * only renders `workspaces` and sends commands; the host does the rest.
 */
export class WorkspaceRemote {
	workspaces = $state.raw<ServerWorkspace[]>([]);
	/** Whether the host answered lately. */
	online = $state(true);
	/** Called after every change to `workspaces`. */
	onChange: (() => void) | null = null;
	private unsubscribe: Unsubscribe | null = null;
	/** Refreshes waiting for the next snapshot. */
	private waiters: (() => void)[] = [];
	private readonly transport: ControlPlaneTransport;

	constructor(transport: ControlPlaneTransport) {
		this.transport = transport;
	}

	/** Keep `workspaces` current (true while the app is in front). */
	follow(on: boolean): void {
		if (!on) {
			this.unsubscribe?.();
			this.unsubscribe = null;
		} else if (!this.unsubscribe) {
			this.unsubscribe = this.transport.subscribeWorkspace({
				snapshot: (s) => {
					this.workspaces = s.workspaces;
					for (const done of this.waiters.splice(0)) done();
					this.onChange?.();
				},
				status: (live) => (this.online = live)
			});
		}
	}

	/** Rejects with the host's reason when it refuses, or when it can't be reached. */
	command(cmd: WorkspaceCommand): Promise<WorkspaceCommandResult> {
		return this.transport.workspaceCommand(cmd);
	}

	/** Catch up now (back from the lock screen, Refresh): a new connection starts with a full snapshot. */
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

	dispose(): void {
		this.follow(false);
		this.onChange = null;
	}
}
