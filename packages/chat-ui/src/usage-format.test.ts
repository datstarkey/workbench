import { describe, expect, it } from 'vitest';
import type { RateLimitInfo, TranscriptMeta } from '@workbench/types';
import {
	formatEventReset,
	metaUsageChips,
	shortenReset,
	usageChips,
	usageLabelForKind,
	usageNames
} from './usage-format';

const LONDON = 'Europe/London';
/** Noon, Oct 1 2026, in London (BST). */
const now = new Date(Date.UTC(2026, 9, 1, 11));
const secs = (utc: number) => utc / 1000;
const today510pm = secs(Date.UTC(2026, 9, 1, 16, 10));
const oct2at9am = secs(Date.UTC(2026, 9, 2, 8));
const clock = { now, timeZone: LONDON };

// Verbatim labels and reset strings from `claude -p /usage` (Claude Code 2.1.286).
const limits = [
	{ label: 'session', percent: 3, resets: 'Oct 1 at 5:10pm (Europe/London)' },
	{ label: 'week (all models)', percent: 89, resets: 'Oct 2 at 9am (Europe/London)' },
	{ label: 'week (Fable)', percent: 0, resets: 'Oct 2 at 9am (Europe/London)' }
];

const event = (kind: string, utilization: number, resetsAt: number | null): RateLimitInfo => ({
	status: 'allowed_warning',
	kind,
	utilization,
	resetsAt
});

describe('usage chips', () => {
	it('maps limits to short chips, drops unused per-model ones and flags high usage', () => {
		expect(usageChips(limits, null, clock)).toEqual([
			{
				label: '5h',
				percent: 3,
				high: false,
				resets: '5:10pm',
				title: '5-hour session: 3% used · resets 5:10pm'
			},
			{
				label: 'Week',
				percent: 89,
				high: true,
				resets: 'Oct 2 at 9am',
				title: 'Weekly limit: 89% used · resets Oct 2 at 9am'
			}
		]);
		expect(usageChips([{ label: 'week (Fable)', percent: 12 }], null, clock)).toEqual([
			{
				label: 'Fable wk',
				percent: 12,
				high: false,
				resets: null,
				title: 'Weekly Fable limit: 12% used'
			}
		]);
		expect(usageChips([], null, clock)).toEqual([]);
	});

	it('lets a live rate_limit_event override its limit, with its own reset time', () => {
		const [session, week] = usageChips(limits, event('five_hour', 0.81, today510pm), clock);
		expect(session).toMatchObject({ label: '5h', percent: 81, high: true, resets: '5:10pm' });
		expect(week.percent).toBe(89);

		const weekly = usageChips(limits, event('seven_day', 0.9, oct2at9am), clock)[1];
		expect(weekly).toMatchObject({ label: 'Week', percent: 90, resets: 'Oct 2 at 9am' });
	});

	it('ignores events without a figure or whose window has already reset', () => {
		const bare = { status: 'allowed', resetsAt: null, kind: null, utilization: null } as const;
		expect(usageChips(limits, bare, clock)[0].percent).toBe(3);
		const expired = event('five_hour', 0.81, secs(Date.UTC(2026, 9, 1, 10)));
		expect(usageChips(limits, expired, clock)[0].percent).toBe(3);
		expect(usageChips(limits, event('five_hour', 0.81, null), clock)[0].percent).toBe(3);
	});

	it('adds a chip for an event naming a limit /usage did not list', () => {
		const opus = usageChips(limits, event('seven_day_opus', 0.5, oct2at9am), clock).at(-1);
		expect(opus).toMatchObject({ label: 'opus wk', percent: 50 });
	});
});

describe('Codex usage chips', () => {
	it('come from the stream, resets given as a time', () => {
		const meta = {
			usageLimits: [
				{ label: 'session', percent: 40, resetsAt: today510pm },
				{ label: 'week (all models)', percent: 85, resetsAt: oct2at9am }
			]
		} as TranscriptMeta;
		expect(metaUsageChips(meta, clock)).toEqual([
			{
				label: '5h',
				percent: 40,
				high: false,
				resets: '5:10pm',
				title: '5-hour session: 40% used · resets 5:10pm'
			},
			{
				label: 'Week',
				percent: 85,
				high: true,
				resets: 'Oct 2 at 9am',
				title: 'Weekly limit: 85% used · resets Oct 2 at 9am'
			}
		]);
		expect(metaUsageChips({ usageLimits: undefined } as TranscriptMeta, clock)).toEqual([]);
		expect(metaUsageChips(null, clock)).toEqual([]);
	});
});

describe('rate_limit_event kinds', () => {
	const labels = ['session', 'week (all models)', 'week (Sonnet only)', 'week (Fable)'];

	it('matches /usage labels by model word, any case and wording', () => {
		expect(usageLabelForKind('five_hour', labels)).toBe('session');
		expect(usageLabelForKind('seven_day', labels)).toBe('week (all models)');
		expect(usageLabelForKind('seven_day_sonnet', labels)).toBe('week (Sonnet only)');
		expect(usageLabelForKind('seven_day_fable', labels)).toBe('week (Fable)');
	});

	it('falls back to a cleaned label of its own', () => {
		expect(usageLabelForKind('seven_day_oauth_apps', labels)).toBe('week (oauth apps)');
		expect(usageLabelForKind('monthly_overage', labels)).toBe('monthly overage');
		expect(usageNames('week (oauth apps)').short).toBe('oauth apps wk');
	});
});

describe('reset times', () => {
	it('drops the zone only when it is the client’s, and the date only when it is today there', () => {
		expect(shortenReset('Oct 1 at 5:10pm (Europe/London)', now, LONDON)).toBe('5:10pm');
		expect(shortenReset('Oct 1 at 5:10pm (Europe/London)', now, 'America/New_York')).toBe(
			'5:10pm (Europe/London)'
		);
		expect(shortenReset('Oct 2 at 9am (Europe/London)', now, 'America/New_York')).toBe(
			'Oct 2 at 9am (Europe/London)'
		);
		// "Today" is the server zone's: noon Oct 1 in London is already Oct 2 in Kiritimati.
		expect(shortenReset('Oct 1 at 9pm (Asia/Tokyo)', now, LONDON)).toBe('9pm (Asia/Tokyo)');
		expect(shortenReset('Oct 2 at 1am (Pacific/Kiritimati)', now, LONDON)).toBe(
			'1am (Pacific/Kiritimati)'
		);
		expect(shortenReset('Oct 1 at 5:10pm (Not/AZone)', now, LONDON)).toBe(
			'Oct 1 at 5:10pm (Not/AZone)'
		);
		expect(shortenReset('Oct 1 at 5:10pm', now, LONDON)).toBe('Oct 1 at 5:10pm');
	});

	it('formats an event reset like /usage: time today, else date and time', () => {
		expect(formatEventReset(today510pm, now, LONDON)).toBe('5:10pm');
		expect(formatEventReset(oct2at9am, now, LONDON)).toBe('Oct 2 at 9am');
	});
});
