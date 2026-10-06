/**
 * Something a task panel reads from the server (its output tail, a subagent's
 * conversation): fetched once, then again every `intervalMs` while the task
 * runs, and once more after it ends for what was written since the last poll.
 */
export class TaskPoll<T> {
	value = $state.raw<T | null>(null);
	loaded = $state(false);
	private readonly fetch: () => Promise<T | null>;
	private readonly live: () => boolean;
	private readonly intervalMs: number;

	constructor(fetch: () => Promise<T | null>, live: () => boolean, intervalMs = 1500) {
		this.fetch = fetch;
		this.live = live;
		this.intervalMs = intervalMs;
	}

	/** Starts polling; returns the stop function. */
	start(): () => void {
		let stopped = false;
		let inFlight = false;
		/** A tick came while a fetch was out: fetch again once it lands, or the final one is lost. */
		let again = false;
		const load = async () => {
			if (inFlight) {
				again = true;
				return;
			}
			inFlight = true;
			try {
				const next = await this.fetch();
				if (stopped) return;
				this.value = next;
				this.loaded = true;
			} finally {
				inFlight = false;
				if (again && !stopped) {
					again = false;
					void load();
				}
			}
		};
		void load();
		let wasLive = this.live();
		const timer = setInterval(() => {
			const live = this.live();
			if (live || wasLive) void load();
			wasLive = live;
		}, this.intervalMs);
		return () => {
			stopped = true;
			clearInterval(timer);
		};
	}
}
