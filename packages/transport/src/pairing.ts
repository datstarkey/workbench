/** Connection details a phone needs to reach a Workbench server. */
export interface PairingInfo {
	/** Server base URL (`http(s)://host[:port]`). */
	url: string;
	token: string;
}

/** Mirrors `workbench_core::token::is_strong`: ≥32 characters, no whitespace. */
export function isStrongToken(token: string): boolean {
	return token.length >= 32 && !/\s/.test(token);
}

/** `workbench://pair?v=1&url=<url>&token=<token>`, the payload of the pairing QR code. */
export function buildPairingUri({ url, token }: PairingInfo): string {
	return `workbench://pair?${new URLSearchParams({ v: '1', url, token })}`;
}

const PAIRING_PREFIX = 'workbench://pair?';
const PAIRING_KEYS = ['v', 'url', 'token'] as const;

/**
 * The query of a pairing URI, or null unless the text starts with exactly
 * `workbench://pair?` and has no fragment. Deliberately not `new URL()`: older
 * Android System WebView parses custom schemes differently (empty host,
 * pathname `//pair`), so the outer URI is matched as a strict prefix.
 */
export function pairingQuery(text: string): URLSearchParams | null {
	const trimmed = text.trim();
	if (!trimmed.startsWith(PAIRING_PREFIX) || trimmed.includes('#')) return null;
	return new URLSearchParams(trimmed.slice(PAIRING_PREFIX.length));
}

/**
 * Parse a scanned pairing code. Returns null for anything that isn't a
 * well-formed v1 pairing URI, so a random QR code can't point the app at a
 * URL carrying credentials, paths or queries, or at a weak token.
 */
export function parsePairingUri(text: string): PairingInfo | null {
	const query = pairingQuery(text);
	if (!query || PAIRING_KEYS.some((key) => query.getAll(key).length !== 1)) return null;
	if (query.get('v') !== '1') return null;

	const url = serverBaseUrl(query.get('url'));
	const token = query.get('token');
	if (!url || !token || !isStrongToken(token)) return null;
	return { url, token };
}

function serverBaseUrl(raw: string | null): string | null {
	if (!raw) return null;
	let url: URL;
	try {
		url = new URL(raw);
	} catch {
		return null;
	}
	const bare =
		(url.protocol === 'http:' || url.protocol === 'https:') &&
		url.hostname !== '' &&
		!url.username &&
		!url.password &&
		url.pathname === '/' &&
		!url.search &&
		!url.hash;
	// `search`/`hash` read as empty for a lone `?` or `#`, so check the raw text too.
	return bare && !/[?#]/.test(raw) ? url.origin : null;
}
