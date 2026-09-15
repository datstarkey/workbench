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

/**
 * Parse a scanned pairing code. Returns null for anything that isn't a
 * well-formed v1 pairing URI, so a random QR code can't point the app at a
 * URL carrying credentials, paths or queries, or at a weak token.
 */
export function parsePairingUri(text: string): PairingInfo | null {
	let uri: URL;
	try {
		uri = new URL(text.trim());
	} catch {
		return null;
	}
	if (uri.protocol !== 'workbench:' || uri.host !== 'pair' || uri.pathname || uri.hash) return null;
	if (uri.searchParams.get('v') !== '1') return null;

	const url = serverBaseUrl(uri.searchParams.get('url'));
	const token = uri.searchParams.get('token');
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
