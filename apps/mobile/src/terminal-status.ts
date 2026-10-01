import { parseTerminalControlFrame } from '@workbench/transport';

export type TerminalStatus =
	| 'connecting'
	| 'open'
	| 'reconnecting'
	| 'taken_over'
	| 'exited'
	| 'revoked';

/**
 * Status implied by a text frame, or null when it isn't a control frame (and so
 * must not be written to xterm either way — control JSON is never output).
 */
export function statusForTextFrame(data: string): TerminalStatus | null {
	const frame = parseTerminalControlFrame(data);
	if (frame?.t === 'takeover') return 'taken_over';
	if (frame?.t === 'exit') return 'exited';
	// Access was withdrawn: not "Take control" or a retry — reattaching would 401.
	if (frame?.t === 'revoked') return 'revoked';
	return null;
}

/**
 * The server closes the socket after a takeover/exit/revoked frame; keep that
 * reason. Anything else is a dropped connection (the phone slept, the network
 * changed) and is worth re-attaching: the shell is still alive on the server.
 */
export function statusOnClose(current: TerminalStatus): TerminalStatus {
	return current === 'taken_over' || current === 'exited' || current === 'revoked'
		? current
		: 'reconnecting';
}

/** Backoff between re-attach attempts: 0.5s, 1s, 2s … capped at 10s. */
export function reconnectDelay(attempt: number): number {
	return Math.min(500 * 2 ** attempt, 10_000);
}
