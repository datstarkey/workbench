// Calls core put to the mode's decider, by tool and input: the decider's
// dialog (`PermissionRequest`) names no call. A rule, the mode or auto mode's
// classifier settles most without one, so only the newest are kept.
// Identical calls made in parallel queue under one key, oldest first; a call
// that goes ahead (`tool.call`) or is denied leaves the queue (`drop`).

export type PendingAsk = { id: string; reason?: string };

const KEPT = 50;

export class PendingAsks {
	private byKey = new Map<string, PendingAsk[]>();
	private size = 0;

	private static key(tool: string, input: unknown) {
		return `${tool}\0${JSON.stringify(input)}`;
	}

	note(tool: string, input: unknown, ask: PendingAsk) {
		const key = PendingAsks.key(tool, input);
		const before = this.byKey.get(key) ?? [];
		const list = [...before.filter((a) => a.id !== ask.id), ask];
		// The newest key goes last, so the oldest is dropped first.
		this.byKey.delete(key);
		this.byKey.set(key, list);
		this.size += list.length - before.length;
		while (this.size > KEPT) {
			const [oldest, asks] = this.byKey.entries().next().value!;
			asks.shift();
			this.size--;
			if (asks.length === 0) this.byKey.delete(oldest);
		}
	}

	/** Call `id` was settled without a dialog. */
	drop(id: string) {
		for (const [key, asks] of this.byKey) {
			const kept = asks.filter((a) => a.id !== id);
			if (kept.length === asks.length) continue;
			this.size -= asks.length - kept.length;
			if (kept.length) this.byKey.set(key, kept);
			else this.byKey.delete(key);
			return;
		}
	}

	/** The oldest pending call a `PermissionRequest` for this tool and input is about. */
	take(tool: string, input: unknown): PendingAsk | undefined {
		const key = PendingAsks.key(tool, input);
		const asks = this.byKey.get(key);
		const ask = asks?.shift();
		if (ask) this.size--;
		if (asks?.length === 0) this.byKey.delete(key);
		return ask;
	}
}
