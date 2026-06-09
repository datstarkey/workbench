// Covers CSI, OSC, and single-char ESC sequences.
// Adapted from common ANSI stripping patterns used by terminal tooling.
// eslint-disable-next-line no-control-regex
const ANSI_RE = /\u001B(?:[@-Z\\-_]|\[[0-?]*[ -/]*[@-~]|\][^\u0007]*(?:\u0007|\u001B\\))/g;

/** Strip ANSI escape codes from a string */
export function stripAnsi(text: string): string {
	return text.replace(ANSI_RE, '');
}

/** Format an ISO timestamp into a short, human-readable date string */
export function formatSessionDate(timestamp: string): string {
	if (!timestamp) return '';
	const date = new Date(timestamp);
	return date.toLocaleString('en-US', {
		month: 'short',
		day: 'numeric',
		hour: 'numeric',
		minute: '2-digit',
		hour12: true
	});
}
