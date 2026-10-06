/** One speech request per composer; leaving the chat discards any late result. */
export class Dictation {
	busy = $state(false);
	error = $state<string | null>(null);
	private disposed = false;

	constructor(private readonly recognize: () => Promise<string | null>) {}

	async start(insert: (text: string) => void | Promise<void>): Promise<void> {
		if (this.busy || this.disposed) return;
		this.busy = true;
		this.error = null;
		try {
			const text = (await this.recognize())?.trim();
			if (text && !this.disposed) await insert(text);
		} catch (e) {
			if (!this.disposed) this.error = e instanceof Error ? e.message : String(e);
		} finally {
			this.busy = false;
		}
	}

	dispose(): void {
		this.disposed = true;
	}
}

/** Insert at the selection, adding spaces only where the surrounding text needs them. */
export function insertDictation(
	draft: string,
	spoken: string,
	start = draft.length,
	end = start
): { text: string; caret: number } {
	const before = draft.slice(0, start);
	const after = draft.slice(end);
	const text = spoken.trim();
	if (!text) return { text: draft, caret: start };
	const left = before && !/\s$/.test(before) ? ' ' : '';
	const right = after && !/^[\s.,!?;:)]/.test(after) ? ' ' : '';
	const inserted = left + text + right;
	return { text: before + inserted + after, caret: before.length + inserted.length };
}
