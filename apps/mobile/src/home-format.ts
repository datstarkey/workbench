import type { AgentSummary } from '@workbench/types';

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
	switch (item.tool) {
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
			return `Wants to use ${item.tool}`;
	}
}

/** Questions, plans and MCP forms need the full view to answer; plain permissions can be answered from home. */
export function answerableFromHome(item: Waiting): boolean {
	return !['AskUserQuestion', 'ExitPlanMode', 'Elicitation'].includes(item.tool);
}
