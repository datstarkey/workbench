import { onMount } from 'svelte';
import { onBackButtonPress } from '@tauri-apps/api/app';

type Register = (handler: () => void) => Promise<() => void>;

/** One native listener, with the most recently opened screen/sheet taking priority. */
export class BackNavigation {
	private handlers: { handler: () => void; priority: number }[] = [];
	private remove: (() => void) | null = null;
	private pending = false;
	constructor(private readonly register: Register) {}

	add(handler: () => void, priority = 0): () => void {
		const entry = { handler, priority };
		this.handlers.push(entry);
		this.handlers.sort((a, b) => a.priority - b.priority);
		void this.sync();
		return () => {
			this.handlers = this.handlers.filter((h) => h !== entry);
			void this.sync();
		};
	}

	private async sync(): Promise<void> {
		if (this.pending) return;
		if (!this.handlers.length) {
			this.remove?.();
			this.remove = null;
			return;
		}
		if (this.remove) return;
		this.pending = true;
		try {
			this.remove = await this.register(() => this.handlers[this.handlers.length - 1]?.handler());
		} catch (e) {
			console.warn('[mobile] back navigation', e);
		} finally {
			this.pending = false;
		}
		if (!this.handlers.length) {
			this.remove?.();
			this.remove = null;
		}
	}
}

const navigation = new BackNavigation(async (handler) => {
	const listener = await onBackButtonPress(handler);
	return () => {
		void listener.unregister();
	};
});

/** Register only while a screen is mounted; no listener on Home leaves native exit intact. */
export function useBack(handler: () => void, priority = 0): void {
	onMount(() => navigation.add(handler, priority));
}
