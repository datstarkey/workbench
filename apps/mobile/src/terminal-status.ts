import { parseTerminalControlFrame } from '@workbench/transport';

export type TerminalStatus = 'connecting' | 'open' | 'closed' | 'taken_over' | 'exited';

/**
 * Status implied by a text frame, or null when it isn't a control frame (and so
 * must not be written to xterm either way — control JSON is never output).
 */
export function statusForTextFrame(data: string): TerminalStatus | null {
	const frame = parseTerminalControlFrame(data);
	if (frame?.t === 'takeover') return 'taken_over';
	if (frame?.t === 'exit') return 'exited';
	return null;
}

/** The server closes the socket after a takeover/exit frame; keep that reason. */
export function statusOnClose(current: TerminalStatus): TerminalStatus {
	return current === 'taken_over' || current === 'exited' ? current : 'closed';
}
