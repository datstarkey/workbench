import { baseName, type AgentSummary } from '@workbench/types';

type Waiting = NonNullable<AgentSummary['waiting']>;

/** "now", "4m", "2h", "3d": how long since `then` (both unix ms). */
export function age(then: number, now: number): string {
	const mins = Math.floor((now - then) / 60_000);
	if (mins < 1) return 'now';
	if (mins < 60) return `${mins}m`;
	const hours = Math.floor(mins / 60);
	return hours < 24 ? `${hours}h` : `${Math.floor(hours / 24)}d`;
}

/** "github.com/o/r": a repo web URL without its scheme. */
export function repoLabel(url: string): string {
	return url.replace(/^https?:\/\//, '');
}

/** What Claude is waiting on, in a few words. */
export function waitingLabel(item: Waiting): string {
	const label = toolLabel(item.tool);
	return item.inTerminal ? `${label} (answer in its terminal)` : label;
}

function toolLabel(tool: string): string {
	switch (tool) {
		case 'AskUserQuestion':
			return 'Has a question';
		case 'ExitPlanMode':
			return 'Plan ready for review';
		case 'Elicitation':
			return 'Needs your input';
		case 'Bash':
			return 'Wants to run a command';
		case 'Edit':
		case 'Write':
		case 'MultiEdit':
		case 'NotebookEdit':
			return 'Wants to edit a file';
		default:
			return `Wants to use ${tool}`;
	}
}

/**
 * Questions, plans and MCP forms need the full view to answer; plain permissions can be
 * answered from home. One the terminal's own dialog asks can only be answered there.
 */
export function answerableFromHome(item: Waiting): boolean {
	return (
		!item.inTerminal && !['AskUserQuestion', 'ExitPlanMode', 'Elicitation'].includes(item.tool)
	);
}

export { baseName };

/** A path with the home directory shown as `~`. */
export function tildePath(path: string): string {
	return path.replace(
		/^(\/Users|\/home)\/(?!Shared(?:\/|$))[^/]+(?=\/|$)|^[A-Za-z]:\\Users\\[^\\]+(?=\\|$)/,
		'~'
	);
}

/** One spelling per folder: `/` separators, no trailing one, a Windows drive path in lower case. */
export function pathKey(path: string): string {
	const key = path.replace(/\\/g, '/').replace(/(?<=.)\/+$/, '');
	return /^[A-Za-z]:\//.test(key) ? key.toLowerCase() : key;
}
