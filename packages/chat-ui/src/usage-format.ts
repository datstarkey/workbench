import type { RateLimitInfo, TranscriptMeta, UsageLimit } from '@workbench/types';

/** Usage at or above this is flagged, so a limit about to bite stands out. */
export const HIGH_USAGE_PERCENT = 80;

/** The `/usage` labels for the rolling 5-hour window and the all-models weekly limit. */
export const SESSION_LIMIT = 'session';
export const WEEK_ALL_MODELS = 'week (all models)';

/** What a `/usage` label is called on a chip and in full: "session" → "5h" / "5-hour session". */
export function usageNames(label: string): { short: string; long: string } {
	if (label === SESSION_LIMIT) return { short: '5h', long: '5-hour session' };
	if (label === WEEK_ALL_MODELS) return { short: 'Week', long: 'Weekly limit' };
	const model = /^week \((.+)\)$/.exec(label)?.[1];
	if (model) return { short: `${model} wk`, long: `Weekly ${model} limit` };
	return { short: label, long: label };
}

/**
 * The `/usage` label a `rate_limit_event` kind reports on, among the labels
 * `/usage` printed: five_hour → "session", seven_day → "week (all models)",
 * seven_day_sonnet → whichever starts "week (sonnet", any case ("week (Sonnet
 * only)"). A kind matching none gets its own label, underscores as spaces.
 */
export function usageLabelForKind(kind: string, labels: string[]): string {
	if (kind === 'five_hour') return SESSION_LIMIT;
	if (kind === 'seven_day') return WEEK_ALL_MODELS;
	const model = /^seven_day_(.+)$/.exec(kind)?.[1]?.replace(/_/g, ' ');
	const prefix = (model ? `week (${model}` : kind.replace(/_/g, ' ')).toLowerCase();
	const match = labels.find((l) => l.toLowerCase().startsWith(prefix));
	return match ?? (model ? `week (${model})` : kind.replace(/_/g, ' '));
}

const clientZone = () => Intl.DateTimeFormat().resolvedOptions().timeZone;

const dayIn = (d: Date, timeZone: string) =>
	d.toLocaleDateString('en-US', { month: 'short', day: 'numeric', timeZone });

/** "5:10pm", "9am": the CLI's style. */
const timeIn = (d: Date, timeZone: string) =>
	d
		.toLocaleTimeString('en-US', { hour: 'numeric', minute: '2-digit', timeZone })
		.replace(':00', '')
		.replace(/\s/g, '')
		.toLowerCase();

/** A `rate_limit_event` reset (unix seconds) in the client's zone: "5:10pm" today, else "Oct 2 at 9am". */
export function formatEventReset(unixSeconds: number, now: Date, timeZone = clientZone()): string {
	const at = new Date(unixSeconds * 1000);
	const time = timeIn(at, timeZone);
	const day = dayIn(at, timeZone);
	return day === dayIn(now, timeZone) ? time : `${day} at ${time}`;
}

/**
 * A `/usage` reset, written in the server machine's zone ("Oct 1 at 5:10pm
 * (Europe/London)"). The zone is dropped only when it is the client's own; the
 * date only when it is today in that zone, so without a zone it stays.
 */
export function shortenReset(resets: string, now: Date, timeZone = clientZone()): string {
	const parts = /^(.*?)\s*\(([^)]+)\)$/.exec(resets);
	if (!parts) return resets;
	const [, when, zone] = parts;
	let today: string | null = null;
	try {
		today = `${dayIn(now, zone)} at `;
	} catch {
		// Not a zone this runtime knows.
	}
	const local = today && when.startsWith(today) ? when.slice(today.length) : when;
	return zone === timeZone ? local : `${local} (${zone})`;
}

export interface UsageChip {
	label: string;
	percent: number;
	high: boolean;
	resets: string | null;
	title: string;
}

/**
 * Chips for the plan's limits: the 5-hour session and weekly limit always,
 * per-model weekly limits once used. `event` (a `rate_limit_event` newer than
 * the `/usage` figures) wins for the limit it names while its window is open.
 */
export function usageChips(
	limits: UsageLimit[],
	event: RateLimitInfo | null = null,
	{ now = new Date(), timeZone = clientZone() }: { now?: Date; timeZone?: string } = {}
): UsageChip[] {
	const merged = limits.map((l) => ({
		label: l.label,
		percent: l.percent,
		resets: l.resets
			? shortenReset(l.resets, now, timeZone)
			: l.resetsAt != null
				? formatEventReset(l.resetsAt, now, timeZone)
				: null
	}));
	if (
		event?.kind &&
		event.utilization != null &&
		event.resetsAt != null &&
		event.resetsAt * 1000 > now.getTime()
	) {
		const live = {
			label: usageLabelForKind(
				event.kind,
				limits.map((l) => l.label)
			),
			percent: Math.round(event.utilization * 100),
			resets: formatEventReset(event.resetsAt, now, timeZone)
		};
		const i = merged.findIndex((l) => l.label === live.label);
		if (i >= 0) merged[i] = live;
		else merged.push(live);
	}
	return merged
		.filter((l) => l.label === SESSION_LIMIT || l.label === WEEK_ALL_MODELS || l.percent > 0)
		.map((l) => {
			const { short, long } = usageNames(l.label);
			return {
				label: short,
				percent: l.percent,
				high: l.percent >= HIGH_USAGE_PERCENT,
				resets: l.resets,
				title: `${long}: ${l.percent}% used${l.resets ? ` · resets ${l.resets}` : ''}`
			};
		});
}

/** Chips from the limits a chat's own stream reports (Codex); Claude's come from {@link usePlanUsage}. */
export function metaUsageChips(
	meta: TranscriptMeta | null,
	clock?: { now?: Date; timeZone?: string }
): UsageChip[] {
	return usageChips(meta?.usageLimits ?? [], null, clock);
}
