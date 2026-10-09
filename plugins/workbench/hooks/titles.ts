// Keeps the session's title fitting the conversation: after an answered turn
// a small model reads the opening prompt, the latest ones and the reply, and
// names it. A title someone gave (`/rename`) is theirs and stays. What it
// knows of a session is kept in the plugin's store under the session's id, so
// a restart (a mode switch, a rewind, the app) carries on where it was.

/** One completion, spelled in the hook (`$.model.complete`). */
export type Complete = (prompt: string) => Promise<string | undefined>;

/** `$.store`, spelled in the hook. */
export interface Store {
	get: (key: string) => Promise<unknown>;
	set: (key: string, value: unknown) => Promise<void>;
	keys?: () => Promise<string[]>;
	delete?: (key: string) => Promise<void>;
}

interface Saved {
	first?: string;
	recent: string[];
	prompts: number;
	named: boolean;
}

const RECENT_PROMPTS = 3;
const PROMPT_CHARS = 600;
const REPLY_CHARS = 800;
const TITLE_CHARS = 60;
/** Every prompt names the session at first, then every `RETITLE_EVERY`th. */
const EARLY_PROMPTS = 3;
const RETITLE_EVERY = 5;
const SESSIONS_KEPT = 200;
const KEY = 'title:';

export const TITLE_SYSTEM =
	'You name coding-assistant conversations for a sidebar. Reply with the title only: ' +
	'3 to 6 words, sentence case, no quotes or trailing punctuation, naming the task ' +
	"(e.g. 'Fix stale chat after tool calls'). If the current title still fits, reply with it unchanged.";

const clip = (text: string, max: number) => (text.length > max ? `${text.slice(0, max)}…` : text);

const fresh = (): Saved => ({ recent: [], prompts: 0, named: false });

export class Titles {
	private session: string | undefined;
	private loading: Promise<void> = Promise.resolve();
	private saved: Saved = fresh();
	private reply = '';
	/** A prompt came since the last title was asked for. */
	private due = false;
	private generating = false;
	/** Bumped when the conversation changes, so a title asked before it is dropped. */
	private epoch = 0;
	/** Generated, and not yet handed to the engine (`sessionTitle`). */
	private pending: string | undefined;

	/** Work on `sessionId`'s state, read from the store the first time. */
	use(sessionId: string, get: Store['get']): Promise<void> {
		if (sessionId !== this.session) {
			this.forget();
			this.session = sessionId;
			this.loading = get(KEY + sessionId)
				.then((value) => {
					if (this.session === sessionId && isSaved(value)) this.saved = value;
				})
				.catch(() => {});
		}
		return this.loading;
	}

	/** `/clear` or `/resume`: another conversation, read afresh on its first use. */
	reset() {
		this.forget();
		this.session = undefined;
	}

	private forget() {
		this.epoch++;
		this.saved = fresh();
		this.reply = '';
		this.due = false;
		this.pending = undefined;
	}

	/** A prompt the person (or chat) sent; Workbench's own prompts are skipped by the caller. */
	notePrompt(text: string) {
		const trimmed = text.trim();
		if (!trimmed || trimmed.startsWith('<command-')) return;
		const s = this.saved;
		s.first ??= trimmed;
		s.recent = [...s.recent, trimmed].slice(-RECENT_PROMPTS);
		s.prompts++;
		this.due = s.prompts <= EARLY_PROMPTS || s.prompts % RETITLE_EVERY === 0;
	}

	noteReply(text: string) {
		if (text.trim()) this.reply = text.trim();
	}

	/** Someone named the session (`/rename <name>`): no more automatic titles for it. */
	async nameByPerson(store: Store) {
		this.saved.named = true;
		this.pending = undefined;
		await this.save(store);
	}

	/** The title to hand the engine with the next prompt, once. */
	takePending(): string | undefined {
		const title = this.pending;
		this.pending = undefined;
		return title;
	}

	/** After an answered turn: a new title, or `undefined` to keep `current`. */
	async retitle(
		current: string | undefined,
		complete: Complete,
		store: Store
	): Promise<string | undefined> {
		const { first, recent, named } = this.saved;
		if (named || this.generating || !this.due || !first) return undefined;
		this.generating = true;
		this.due = false;
		const epoch = this.epoch;
		try {
			const later = recent.filter((p) => p !== first);
			const prompt = [
				`Current title: ${current ?? '(none)'}`,
				`Opening request:\n${clip(first, PROMPT_CHARS)}`,
				later.length > 0 &&
					`Latest requests:\n${later.map((p) => `- ${clip(p, PROMPT_CHARS)}`).join('\n')}`,
				this.reply && `Latest reply:\n${clip(this.reply, REPLY_CHARS)}`
			]
				.filter(Boolean)
				.join('\n\n');
			const title = clean(await complete(prompt).catch(() => undefined));
			// The conversation changed, or a rename landed, while the model answered.
			if (epoch !== this.epoch) return undefined;
			await this.save(store, true);
			if (!title || this.saved.named || title === current) return undefined;
			this.pending = title;
			return title;
		} finally {
			this.generating = false;
		}
	}

	private async save(store: Store, prune = false) {
		const session = this.session;
		if (!session) return;
		try {
			await store.set(KEY + session, this.saved);
			if (!prune || !store.keys || !store.delete) return;
			const keys = (await store.keys()).filter((k) => k.startsWith(KEY));
			for (const key of keys.slice(0, Math.max(0, keys.length - SESSIONS_KEPT)))
				await store.delete(key);
		} catch {
			// A store that can't be written costs only the memory across restarts.
		}
	}
}

function isSaved(value: unknown): value is Saved {
	const v = value as Saved | null;
	return (
		typeof v === 'object' &&
		v !== null &&
		Array.isArray(v.recent) &&
		typeof v.prompts === 'number' &&
		typeof v.named === 'boolean'
	);
}

/** The first line, without wrapping quotes or a closing full stop, cut at a word. */
export function clean(text: string | undefined): string | undefined {
	const line = text?.trim().split('\n')[0] ?? '';
	const title = line
		.replace(/^["'`“”‘’]+|["`“”‘’]+$/g, '')
		.replace(/\.+$/, '')
		.trim();
	if (!title) return undefined;
	if (title.length <= TITLE_CHARS) return title;
	const cut = title.slice(0, TITLE_CHARS);
	const space = cut.lastIndexOf(' ');
	return space > 0 ? cut.slice(0, space) : cut;
}
