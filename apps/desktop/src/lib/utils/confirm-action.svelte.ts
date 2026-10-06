/** Reusable pattern for confirm-before-action workflows */
export class ConfirmAction<T> {
	error = $state('');
	busy = $state(false);
	#open = $state(false);
	private pending: T | null = $state(null);

	/** Dismissal (Escape, outside click, Cancel) is ignored while the action runs. */
	get open() {
		return this.#open;
	}

	set open(value: boolean) {
		if (!this.busy) this.#open = value;
	}

	get pendingValue(): T | null {
		return this.pending;
	}

	request(value: T) {
		if (this.busy) return;
		this.pending = value;
		this.error = '';
		this.#open = true;
	}

	async confirm(action: (value: T) => Promise<void>) {
		if (!this.pending || this.busy) return;
		this.error = '';
		this.busy = true;
		try {
			await action(this.pending);
			this.pending = null;
			this.#open = false;
		} catch (e) {
			this.error = String(e);
		} finally {
			this.busy = false;
		}
	}

	cancel() {
		if (this.busy) return;
		this.pending = null;
		this.error = '';
		this.#open = false;
	}
}
