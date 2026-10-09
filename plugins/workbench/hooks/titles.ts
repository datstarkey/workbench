// Keeps the session's title fitting the conversation: after each answered
// turn a small model reads the opening prompt, the latest ones and the reply,
// and names it. A title someone gave (`/rename`) is theirs and stays.

/** One completion, spelled in the hook (`$.model.complete`). */
export type Complete = (prompt: string) => Promise<string | undefined>;

const RECENT_PROMPTS = 3;
const PROMPT_CHARS = 600;
const REPLY_CHARS = 800;
const TITLE_CHARS = 60;

export const TITLE_SYSTEM =
	'You name coding-assistant conversations for a sidebar. Reply with the title only: ' +
	'3 to 6 words, sentence case, no quotes or trailing punctuation, naming the task ' +
	"(e.g. 'Fix stale chat after tool calls'). If the current title still fits, reply with it unchanged.";

const clip = (text: string, max: number) => (text.length > max ? `${text.slice(0, max)}…` : text);

export class Titles {
	private first: string | undefined;
	private recent: string[] = [];
	private reply = '';
	/** A prompt came since the last title was asked for. */
	private fresh = false;
	private named = false;
	private generating = false;
	/** Generated, and not yet handed to the engine (`sessionTitle`). */
	private pending: string | undefined;

	/** A prompt the person (or chat) sent; Workbench's own prompts are skipped by the caller. */
	notePrompt(text: string) {
		const trimmed = text.trim();
		if (!trimmed || trimmed.startsWith('<')) return;
		this.first ??= trimmed;
		this.recent = [...this.recent, trimmed].slice(-RECENT_PROMPTS);
		this.fresh = true;
	}

	noteReply(text: string) {
		if (text.trim()) this.reply = text.trim();
	}

	/** Someone named the session (`/rename`): no more automatic titles for it. */
	nameByPerson() {
		this.named = true;
		this.pending = undefined;
	}

	/** `/clear` or `/resume`: another conversation, titled afresh. */
	reset() {
		Object.assign(this, new Titles());
	}

	/** The title to hand the engine with the next prompt, once. */
	takePending(): string | undefined {
		const title = this.pending;
		this.pending = undefined;
		return title;
	}

	/** After an answered turn: a new title, or `undefined` to keep `current`. */
	async retitle(current: string | undefined, complete: Complete): Promise<string | undefined> {
		if (this.named || this.generating || !this.fresh || !this.first) return undefined;
		this.generating = true;
		this.fresh = false;
		try {
			const earlier = this.recent.filter((p) => p !== this.first);
			const prompt = [
				`Current title: ${current ?? '(none)'}`,
				`Opening request:\n${clip(this.first, PROMPT_CHARS)}`,
				earlier.length > 0 &&
					`Latest requests:\n${earlier.map((p) => `- ${clip(p, PROMPT_CHARS)}`).join('\n')}`,
				this.reply && `Latest reply:\n${clip(this.reply, REPLY_CHARS)}`
			]
				.filter(Boolean)
				.join('\n\n');
			const title = clean(await complete(prompt).catch(() => undefined));
			// A rename may have landed while the model answered.
			if (!title || this.named || title === current) return undefined;
			this.pending = title;
			return title;
		} finally {
			this.generating = false;
		}
	}
}

function clean(text: string | undefined): string | undefined {
	const line = text?.trim().split('\n')[0]?.trim();
	const title = line?.replace(/^["'`]+|["'`.]+$/g, '').trim();
	return title ? clip(title, TITLE_CHARS) : undefined;
}
