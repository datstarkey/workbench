/** Reusable pattern for confirm-before-action workflows */
export class ConfirmAction<T> {
	open = $state(false);
	error = $state('');
	private pending: T | null = null;

	get pendingValue(): T | null {
		return this.pending;
	}

	request(value: T) {
		this.pending = value;
		this.error = '';
		this.open = true;
	}

	async confirm(action: (value: T) => Promise<void>) {
		if (!this.pending) return;
		this.error = '';
		try {
			await action(this.pending);
			this.pending = null;
			this.open = false;
		} catch (e) {
			this.error = String(e);
		}
	}

	cancel() {
		this.pending = null;
		this.error = '';
		this.open = false;
	}
}
