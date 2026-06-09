import * as Sentry from '@sentry/svelte';

/**
 * Guards against an xterm.js + WKWebView (Tauri/macOS) input-duplication bug.
 *
 * Symptom: while typing — especially with Shift held or Caps Lock on — the
 * hidden `<textarea>` xterm uses for input occasionally emits the *whole*
 * recently-typed run a second time as a single `insertText` blob (often
 * flushed when the space bar ends a batch). Each character is delivered once
 * per keystroke, then the blob re-delivers the run, so the PTY/TUI sees the
 * text twice with extra whitespace. This does not happen in Chromium-based
 * terminals (VS Code/Electron) because WKWebView's composition/`input` event
 * timing differs.
 *
 * We can't patch xterm's DOM handlers safely, so we dedup at the `onData`
 * boundary we own: if a single multi-character, printable-only `onData` payload
 * exactly reproduces characters we *just* emitted one-by-one, it's the spurious
 * flush — drop it. Control sequences (ESC/CR/LF/tab/DEL — arrow keys,
 * Shift+Enter, bracketed paste, backspace) never match, and a recent real
 * `paste` disables dedup, so legitimate input is never dropped.
 *
 * Every drop is the fix *and* the diagnostic: it reports a content-free shape
 * to Sentry (prod) and warns to the console (dev) so we can confirm the fix is
 * firing and catch any residual cases in the wild.
 */

/** Only chars in this range count as "printable typed text" for matching. */
function isPrintable(ch: string): boolean {
	const code = ch.charCodeAt(0);
	return code >= 0x20 && code !== 0x7f;
}

/** True if every char of `s` is printable (space included), none control. */
function allPrintable(s: string): boolean {
	for (const ch of s) {
		const code = ch.charCodeAt(0);
		if (code < 0x20 || code === 0x7f) return false;
	}
	return true;
}

const MATCH_WINDOW_MS = 4000;
const PASTE_GRACE_MS = 1500;
const HISTORY_CAP = 256;
const SENTRY_THROTTLE_MS = 60_000;

interface TypedChar {
	ch: string;
	t: number;
}

export interface DuplicateShape {
	dataLength: number;
	spaceCount: number;
	maxSpaceRun: number;
	distinctChars: number;
	historyLength: number;
}

export class TerminalInputDedup {
	private history: TypedChar[] = [];
	// -Infinity so "no paste yet" / "never reported" never reads as recent,
	// even when performance.now() is still small early in the app's life.
	private lastPasteAt = Number.NEGATIVE_INFINITY;
	private lastSentryAt = Number.NEGATIVE_INFINITY;
	private suppressedSinceReport = 0;

	constructor(
		private readonly sessionId: string,
		private readonly paneType: () => string | null
	) {}

	/** Call when a real clipboard paste occurs — disables dedup briefly. */
	notePaste(now: number): void {
		this.lastPasteAt = now;
	}

	/**
	 * Decide whether an `onData` payload is the spurious duplicate flush.
	 * When true, the caller drops the payload (does not write it to the PTY).
	 * Reporting (Sentry + console) happens here as a side effect of detection.
	 */
	isDuplicateFlush(data: string, now: number): boolean {
		if (data.length < 2) return false;
		if (!allPrintable(data)) return false;
		if (now - this.lastPasteAt < PASTE_GRACE_MS) return false;

		this.prune(now);
		const recent = this.history.map((h) => h.ch).join('');
		if (recent.length < data.length) return false;
		if (!recent.includes(data)) return false;

		this.report(data, recent.length, now);
		return true;
	}

	/** Record a delivered payload so later flushes can be matched against it. */
	recordSent(data: string, now: number): void {
		if (data.length !== 1) return;
		if (!isPrintable(data)) return;
		this.history.push({ ch: data, t: now });
		if (this.history.length > HISTORY_CAP) {
			this.history.splice(0, this.history.length - HISTORY_CAP);
		}
	}

	private prune(now: number): void {
		const cutoff = now - MATCH_WINDOW_MS;
		let i = 0;
		while (i < this.history.length && this.history[i].t < cutoff) i++;
		if (i > 0) this.history.splice(0, i);
	}

	private report(data: string, historyLength: number, now: number): void {
		const shape = describeShape(data, historyLength);
		console.warn(
			`[terminal] dropped duplicate input flush (session ${this.sessionId.slice(0, 8)})`,
			shape
		);
		this.suppressedSinceReport++;
		if (now - this.lastSentryAt < SENTRY_THROTTLE_MS) return;
		const suppressed = this.suppressedSinceReport;
		this.lastSentryAt = now;
		this.suppressedSinceReport = 0;
		Sentry.withScope((scope) => {
			scope.setLevel('warning');
			scope.setTag('component', 'terminal');
			scope.setTag('paneType', this.paneType() ?? 'shell');
			scope.setContext('duplicateFlush', {
				...shape,
				suppressedSinceLastReport: suppressed
			});
			Sentry.captureMessage('terminal.input.duplicate_flush');
		});
	}
}

function describeShape(data: string, historyLength: number): DuplicateShape {
	let spaceCount = 0;
	let maxSpaceRun = 0;
	let run = 0;
	for (const ch of data) {
		if (ch === ' ') {
			spaceCount++;
			run++;
			if (run > maxSpaceRun) maxSpaceRun = run;
		} else {
			run = 0;
		}
	}
	return {
		dataLength: data.length,
		spaceCount,
		maxSpaceRun,
		distinctChars: new Set(data).size,
		historyLength
	};
}
