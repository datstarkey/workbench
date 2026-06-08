import { createHttpTransport } from '@workbench/transport';
import { ControlPlaneStore } from '@workbench/control-plane-ui';
import { uid } from '$lib/utils/uid';

export type InstanceStatus = 'connecting' | 'online' | 'offline';

export interface RemoteInstanceConfig {
	id: string;
	name: string;
	url: string;
	token?: string;
}

/** A connected remote Workbench server: its config, live status, and a
 *  transport-driven control-plane store for its projects/sessions. */
export class RemoteInstance {
	readonly config: RemoteInstanceConfig;
	status = $state<InstanceStatus>('connecting');
	readonly store: ControlPlaneStore;
	private loadedOnce = false;

	constructor(config: RemoteInstanceConfig) {
		this.config = config;
		this.store = new ControlPlaneStore(
			createHttpTransport({ baseUrl: config.url, token: config.token })
		);
	}

	get base(): string {
		return this.config.url.replace(/\/$/, '');
	}

	/** Ping /health; on first transition to online, load its projects/sessions. */
	async checkHealth(): Promise<void> {
		try {
			const headers: Record<string, string> = {};
			if (this.config.token) headers.authorization = `Bearer ${this.config.token}`;
			const res = await fetch(`${this.base}/health`, { headers });
			this.status = res.ok ? 'online' : 'offline';
		} catch {
			this.status = 'offline';
		}
		if (this.status === 'online' && !this.loadedOnce) {
			this.loadedOnce = true;
			await this.store.refresh();
		}
	}
}

const STORAGE_KEY = 'workbench.instances';

/**
 * Owns the set of instances shown in the left sidebar: the implicit local
 * instance ("This Mac") plus connected remote servers. Remotes are persisted to
 * localStorage and health-polled.
 */
export class InstancesStore {
	readonly localId = 'local';
	localName = $state('This Mac');
	remotes = $state<RemoteInstance[]>([]);
	activeId = $state<string>('local');

	private pollHandle: ReturnType<typeof setInterval> | null = null;

	get activeIsLocal(): boolean {
		return this.activeId === this.localId;
	}

	get activeRemote(): RemoteInstance | undefined {
		return this.remotes.find((r) => r.config.id === this.activeId);
	}

	load(): void {
		let configs: RemoteInstanceConfig[];
		try {
			configs = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? '[]');
		} catch {
			configs = [];
		}
		this.remotes = configs.map((c) => new RemoteInstance(c));
		void this.pollAll();
		this.startPolling();
	}

	private persist(): void {
		localStorage.setItem(STORAGE_KEY, JSON.stringify(this.remotes.map((r) => r.config)));
	}

	add(config: Omit<RemoteInstanceConfig, 'id'>): RemoteInstance {
		const instance = new RemoteInstance({ ...config, id: uid() });
		this.remotes = [...this.remotes, instance];
		this.persist();
		void instance.checkHealth();
		return instance;
	}

	remove(id: string): void {
		this.remotes = this.remotes.filter((r) => r.config.id !== id);
		if (this.activeId === id) this.activeId = this.localId;
		this.persist();
	}

	setActive(id: string): void {
		this.activeId = id;
		const remote = this.remotes.find((r) => r.config.id === id);
		if (remote) void remote.checkHealth();
	}

	private async pollAll(): Promise<void> {
		await Promise.all(this.remotes.map((r) => r.checkHealth()));
	}

	private startPolling(): void {
		if (this.pollHandle) return;
		this.pollHandle = setInterval(() => void this.pollAll(), 15000);
	}
}
