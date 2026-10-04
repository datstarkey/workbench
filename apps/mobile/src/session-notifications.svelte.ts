import { invoke, isTauri, addPluginListener } from '@tauri-apps/api/core';
import type { AgentSummary } from '@workbench/types';
import { lsGet, lsSet } from './storage';

export interface NotificationConnection {
	url: string;
	token: string;
	machineId: string;
	name: string;
}
export interface NotificationApi {
	start(connection: NotificationConnection): Promise<void>;
	stop(): Promise<void>;
	takeOpen(): Promise<{ session: string | null }>;
	listen(handler: () => void): Promise<() => void>;
}

const native: NotificationApi = {
	start: (args) => invoke('plugin:session-notifications|start', { ...args }),
	stop: () => invoke('plugin:session-notifications|stop'),
	takeOpen: () => invoke('plugin:session-notifications|take_open_session'),
	async listen(handler) {
		const listener = await addPluginListener('session-notifications', 'open', handler);
		return () => {
			void listener.unregister();
		};
	}
};

/** Serialise native reconfiguration so a late start cannot keep monitoring an old machine. */
export class SessionNotifications {
	enabled = $state(lsGet('wb.notifications') === 'true');
	error = $state<string | null>(null);
	private revision = 0;
	private queue: Promise<void> = Promise.resolve();
	constructor(
		private readonly api: NotificationApi = native,
		readonly supported = isTauri() && /Android/i.test(navigator.userAgent)
	) {}

	setEnabled(enabled: boolean): void {
		this.enabled = enabled;
		lsSet('wb.notifications', String(enabled));
	}

	configure(connection: NotificationConnection | null): Promise<void> {
		const revision = ++this.revision;
		const enabled = this.enabled;
		this.queue = this.queue.then(async () => {
			if (!this.supported || revision !== this.revision) return;
			try {
				if (enabled && connection) await this.api.start(connection);
				else await this.api.stop();
				if (enabled && connection && revision === this.revision) this.error = null;
			} catch (e) {
				if (revision !== this.revision) return;
				this.error = e instanceof Error ? e.message : String(e);
				this.setEnabled(false);
			}
		});
		return this.queue;
	}

	async listen(
		open: (machineId: string, chat: AgentSummary) => Promise<void>
	): Promise<() => void> {
		if (!this.supported) return () => {};
		let closed = false;
		const take = async () => {
			try {
				const { session } = await this.api.takeOpen();
				if (!session || closed) return;
				const payload = JSON.parse(session);
				if (typeof payload.machineId === 'string' && typeof payload.chat?.sessionId === 'string')
					await open(payload.machineId, payload.chat);
			} catch (e) {
				if (!closed) this.error = e instanceof Error ? e.message : String(e);
			}
		};
		const remove = await this.api.listen(() => {
			void take();
		});
		void take();
		return () => {
			closed = true;
			remove();
		};
	}
}
