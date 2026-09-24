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

/** Compact age of an ISO timestamp: "now", "5m", "3h", "2d", "4w", then "Mar 3" */
export function formatRelativeTime(timestamp: string, now = Date.now()): string {
	const then = new Date(timestamp).getTime();
	if (isNaN(then)) return '';
	const minutes = Math.floor((now - then) / 60_000);
	if (minutes < 1) return 'now';
	if (minutes < 60) return `${minutes}m`;
	const hours = Math.floor(minutes / 60);
	if (hours < 24) return `${hours}h`;
	const days = Math.floor(hours / 24);
	if (days < 7) return `${days}d`;
	if (days < 30) return `${Math.floor(days / 7)}w`;
	return new Date(then).toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
}

/** "1 file", "3 files" */
export function plural(count: number, word: string): string {
	return `${count} ${word}${count === 1 ? '' : 's'}`;
}
