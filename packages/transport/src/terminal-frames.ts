/**
 * Server → client control frames on the terminal WebSocket (text JSON; PTY
 * output arrives as binary frames).
 */
export type TerminalControlFrame =
	/** Another client attached; this socket is about to be closed. */
	| { t: 'takeover' }
	/** The shell exited; `code` is null when the status wasn't available. */
	| { t: 'exit'; code: number | null }
	/** The listener this socket used stopped (server mode off / token rotated). */
	| { t: 'revoked' };

/** Parse a text frame into a control frame, or null for anything else. */
export function parseTerminalControlFrame(data: string): TerminalControlFrame | null {
	let msg: unknown;
	try {
		msg = JSON.parse(data);
	} catch {
		return null;
	}
	if (typeof msg !== 'object' || msg === null) return null;
	const { t, code } = msg as { t?: unknown; code?: unknown };
	if (t === 'takeover' || t === 'revoked') return { t };
	if (t === 'exit') return { t, code: typeof code === 'number' ? code : null };
	return null;
}
