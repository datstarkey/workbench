export interface TextPart {
	text: string;
	match: boolean;
}

/** `text` split around every case-insensitive occurrence of `query`, for highlighting. */
export function matchParts(text: string, query: string): TextPart[] {
	const q = query.trim().toLowerCase();
	const lower = text.toLowerCase();
	// Offsets in `lower` only map onto `text` when lowercasing keeps the length.
	if (!q || lower.length !== text.length) return [{ text, match: false }];
	const parts: TextPart[] = [];
	let from = 0;
	for (let at = lower.indexOf(q); at !== -1; at = lower.indexOf(q, from)) {
		if (at > from) parts.push({ text: text.slice(from, at), match: false });
		parts.push({ text: text.slice(at, at + q.length), match: true });
		from = at + q.length;
	}
	if (from < text.length) parts.push({ text: text.slice(from), match: false });
	return parts;
}
