import { describe, expect, it } from 'vitest';
import type { AgentSummary } from '@workbench/types';
import { age, answerableFromHome, waitingLabel } from './home-format.ts';

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
	});
});
