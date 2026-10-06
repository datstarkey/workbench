/** A `@` or `/` word being typed: where its sigil is and the text after it. */
export interface CaretToken {
	start: number;
	query: string;
}

/**
 * The `<sigil>word` ending at the caret, at the start or after whitespace; a
 * second sigil ends it (`a@b.com` and `src/x` are not tokens).
 */
export function tokenAtCaret(draft: string, caret: number, sigil: '@' | '/'): CaretToken | null {
	const before = draft.slice(0, caret);
	const match = new RegExp(`(^|\\s)${sigil}([^\\s${sigil}]*)$`).exec(before);
	if (!match) return null;
	return { start: before.length - match[2].length - 1, query: match[2] };
}

/** Replace the token typed from `start` (the rest of its word included) with `text`. */
export function replaceToken(
	draft: string,
	start: number,
	caret: number,
	text: string
): { text: string; caret: number } {
	const after = draft.slice(caret).replace(/^\S*/, '').replace(/^ /, '');
	return { text: draft.slice(0, start) + text + after, caret: start + text.length };
}
