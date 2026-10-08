import { createHttpTransport, DEFAULT_TIMEOUT_MS, withTimeout } from '@workbench/transport';
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
 *  transport-driven control-plane store for its projects. */
/** A server's `/health`, with its token if it has one, giving up after 10s. */
export function probeHealth(base: string, token?: string): Promise<Response> {
	const headers: Record<string, string> = token ? { authorization: `Bearer ${token}` } : {};
	return withTimeout('GET /health', DEFAULT_TIMEOUT_MS, (signal) =>
		fetch(`${base}/health`, { headers, signal })
	);
}

export class RemoteInstance {
	readonly config: RemoteInstanceConfig;
	status = $state<InstanceStatus>('connecting');
	readonly store: ControlPlaneStore;

	constructor(config: RemoteInstanceConfig) {
		this.config = config;
		this.store = new ControlPlaneStore(
			createHttpTransport({ baseUrl: config.url, token: config.token })
		);
	}

	get base(): string {
		return this.config.url.replace(/\/$/, '');
	}

	/** Ping /health; (re)load its projects on any transition into online —
	 *  the first connect AND every recovery after an outage, so a server that comes
	 *  back doesn't keep showing stale pre-outage data. */
	async checkHealth(): Promise<void> {
		const prev = this.status;
		try {
			const res = await probeHealth(this.base, this.config.token);
			this.status = res.ok ? 'online' : 'offline';
		} catch {
			this.status = 'offline';
		}
		if (this.status === 'online' && prev !== 'online') {
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
		let configs: RemoteInstanceConfig[] = [];
		try {
			const parsed = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? '[]');
			// Guard against corrupt / schema-drifted localStorage: a non-array would
			// otherwise blow up the .map below (outside any try/catch) at startup.
			if (Array.isArray(parsed)) configs = parsed;
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

	/** Stop health polling. */
	dispose(): void {
		if (this.pollHandle) {
			clearInterval(this.pollHandle);
			this.pollHandle = null;
		}
	}
}
