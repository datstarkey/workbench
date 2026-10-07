import type { ControlPlaneTransport } from '@workbench/transport';
import type { HostUpdateStatus } from '@workbench/types';

/** The connected host's Workbench version, and installing its update from the phone. */
export class HostUpdate {
	/** Null until the host answers, and for good on one that can't update itself (501). */
	status = $state.raw<HostUpdateStatus | null>(null);
	/** The version this phone asked the host to install; the host restarts on it. */
	target = $state<string | null>(null);
	error = $state<string | null>(null);

	/** Also an install started elsewhere (the desktop itself, another phone). */
	updating = $derived(this.target !== null || this.status?.installing === true);

	/** The host's version when the install started: any other version is success. */
	private from: string | null = null;
	/** Bumped when an install starts, so a check sent before it can't settle it. */
	private generation = 0;

	private readonly transport: ControlPlaneTransport;

	constructor(transport: ControlPlaneTransport) {
		this.transport = transport;
	}

	/** Also how an install is seen to end: the host answers on another version, or stops installing. */
	async check(): Promise<void> {
		const generation = this.generation;
		let status: HostUpdateStatus;
		try {
			status = await this.transport.invoke('host_update_status', undefined);
		} catch (e) {
			if ((e as { status?: number }).status === 501) this.status = null;
			// Otherwise restarting into the update, or offline: keep what we know.
			return;
		}
		if (generation !== this.generation) return;
		this.status = status;
		this.error = null;
		if (!this.target) return;
		if (status.current !== this.from) this.target = null;
		else if (!status.installing) {
			this.target = null;
			this.error = "The host couldn't install the update. Try again, or update it on the desktop.";
		}
	}

	async install(): Promise<void> {
		const current = this.status?.current;
		if (!current || !this.status?.available || this.updating) return;
		this.error = null;
		try {
			const { version } = await this.transport.invoke('host_update_install', undefined);
			this.generation++;
			this.from = current;
			this.target = version;
		} catch (e) {
			this.error = `Couldn't update the host: ${e instanceof Error ? e.message : String(e)}`;
		}
	}
}
