/** An `@` mention being typed: where its `@` is and the text after it. */
export interface MentionQuery {
	start: number;
	query: string;
}

/** The `@path` word ending at the caret, if one is being typed. */
export function mentionQuery(draft: string, caret: number): MentionQuery | null {
	const before = draft.slice(0, caret);
	const match = /(^|\s)@([^\s@]*)$/.exec(before);
	if (!match) return null;
	return { start: before.length - match[2].length - 1, query: match[2] };
}

/**
 * Paths for the `@` menu, best first: file-name prefix, file-name substring,
 * path substring, then the query's characters in order (`chcomp` →
 * `chat-ui/src/ChatComposer.svelte`). Shorter paths win ties.
 */
export function matchFiles(paths: string[], query: string, limit = 50): string[] {
	const q = query.toLowerCase();
	if (!q) return paths.slice(0, limit);
	const rank = (path: string) => {
		const lower = path.toLowerCase();
		const name = lower.slice(lower.lastIndexOf('/') + 1);
		if (name.startsWith(q)) return 0;
		if (name.includes(q)) return 1;
		if (lower.includes(q)) return 2;
		return inOrder(lower, q) ? 3 : -1;
	};
	return paths
		.map((path) => ({ path, r: rank(path) }))
		.filter(({ r }) => r >= 0)
		.sort((a, b) => a.r - b.r || a.path.length - b.path.length || a.path.localeCompare(b.path))
		.slice(0, limit)
		.map(({ path }) => path);
}

function inOrder(text: string, chars: string): boolean {
	let at = 0;
	for (const c of chars) {
		at = text.indexOf(c, at) + 1;
		if (at === 0) return false;
	}
	return true;
}

/**
 * Replace the mention being typed with `@path ` (quoted when the path has
 * spaces, which Claude Code reads as one mention); returns the new caret too.
 */
export function insertMention(
	draft: string,
	mention: MentionQuery,
	caret: number,
	path: string
): { text: string; caret: number } {
	const token = /\s/.test(path) ? `@"${path}" ` : `@${path} `;
	const after = draft.slice(caret).replace(/^\S*/, '').replace(/^ /, '');
	const text = draft.slice(0, mention.start) + token + after;
	return { text, caret: mention.start + token.length };
}
