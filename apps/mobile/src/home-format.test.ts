import { describe, expect, it } from 'vitest';
import type { AgentSummary } from '@workbench/types';
import {
	age,
	answerableFromHome,
	pathKey,
	repoLabel,
	runningTasksLabel,
	tildePath,
	totalRunningTasks,
	waitingLabel
} from './home-format.ts';

describe('runningTasksLabel', () => {
	it('names what runs and leaves out what does not', () => {
		expect(runningTasksLabel({ agents: 2, tasks: 1 })).toBe('2 agents · 1 task');
		expect(runningTasksLabel({ agents: 1, tasks: 0 })).toBe('1 agent');
		expect(runningTasksLabel({ agents: 0, tasks: 3 })).toBe('3 tasks');
	});

	it('is null when nothing runs or an older host sent no counts', () => {
		expect(runningTasksLabel({ agents: 0, tasks: 0 })).toBeNull();
		expect(runningTasksLabel(undefined)).toBeNull();
	});

	it('adds up every pane, missing counts as none', () => {
		expect(
			totalRunningTasks([{ agents: 1, tasks: 2 }, undefined, { agents: 3, tasks: 0 }])
		).toEqual({ agents: 4, tasks: 2 });
	});
});

const approval = (tool: string): NonNullable<AgentSummary['waiting']> => ({
	id: 'r1',
	tool,
	preview: ''
});

describe('age', () => {
	it('rounds down to minutes, hours, then days', () => {
		const now = 10 * 86_400_000;
		expect(age(now - 30_000, now)).toBe('now');
		expect(age(now - 12 * 60_000, now)).toBe('12m');
		expect(age(now - 3 * 3_600_000, now)).toBe('3h');
		expect(age(now - 50 * 3_600_000, now)).toBe('2d');
	});
});

describe('repoLabel', () => {
	it('drops the scheme', () => {
		expect(repoLabel('https://github.com/o/r')).toBe('github.com/o/r');
	});
});

describe('waiting on you', () => {
	it('names the kind of request', () => {
		expect(waitingLabel(approval('Bash'))).toBe('Wants to run a command');
		expect(waitingLabel(approval('Edit'))).toBe('Wants to edit a file');
		expect(waitingLabel(approval('AskUserQuestion'))).toBe('Has a question');
		expect(waitingLabel(approval('WebFetch'))).toBe('Wants to use WebFetch');
	});

	it('sends questions and plans to the full view', () => {
		expect(answerableFromHome(approval('Bash'))).toBe(true);
		expect(answerableFromHome(approval('AskUserQuestion'))).toBe(false);
		expect(answerableFromHome(approval('ExitPlanMode'))).toBe(false);
		expect(answerableFromHome(approval('Elicitation'))).toBe(false);
		expect(waitingLabel(approval('Elicitation'))).toBe('Needs your input');
	});

	it("leaves one the terminal's own dialog asks to the terminal", () => {
		const inTerminal = { ...approval('Bash'), inTerminal: true };
		expect(answerableFromHome(inTerminal)).toBe(false);
		expect(waitingLabel(inTerminal)).toBe('Wants to run a command (answer in its terminal)');
	});
});

describe('tildePath', () => {
	it('shows the home directory as ~', () => {
		expect(tildePath('/Users/jake/Repos/app')).toBe('~/Repos/app');
		expect(tildePath('/home/jake')).toBe('~');
		expect(tildePath('C:\\Users\\jake\\src')).toBe('~\\src');
		expect(tildePath('/srv/Users/jake')).toBe('/srv/Users/jake');
		expect(tildePath('/Users/Shared/app')).toBe('/Users/Shared/app');
	});
});

describe('pathKey', () => {
	it('spells one folder one way', () => {
		expect(pathKey('/r/app/')).toBe('/r/app');
		expect(pathKey('/')).toBe('/');
		expect(pathKey('C:\\R\\App\\')).toBe('c:/r/app');
		expect(pathKey('C:/r/app')).toBe('c:/r/app');
	});
});
