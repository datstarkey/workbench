/**
 * Lets a chat view type into the terminal of the pane it mirrors. TerminalPane
 * registers its xterm; input goes through xterm (not straight to the PTY) so it
 * takes the same path as keystrokes: bracketed paste, activity tracking, dedup.
 */
export interface PaneInput {
	/** Paste text as one block (multi-line safe when the app enables bracketed paste). */
	paste(text: string): void;
	/** Send raw key data, e.g. `\r` for Enter or `\x1b` for Escape. */
	key(data: string): void;
}

const inputs = new Map<string, PaneInput>();

/** Returns an unregister function that only removes this exact registration. */
export function registerPaneInput(paneId: string, input: PaneInput): () => void {
	inputs.set(paneId, input);
	return () => {
		if (inputs.get(paneId) === input) inputs.delete(paneId);
	};
}

export function paneInput(paneId: string): PaneInput | undefined {
	return inputs.get(paneId);
}

/** Enter, sent a beat after a paste so the TUI has consumed the paste first. */
const SUBMIT_DELAY_MS = 40;

/** Type a prompt into the pane's Claude session and submit it. */
export function submitPrompt(paneId: string, text: string): boolean {
	const input = inputs.get(paneId);
	if (!input) return false;
	input.paste(text);
	setTimeout(() => input.key('\r'), SUBMIT_DELAY_MS);
	return true;
}
