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
	resetsAt: number | null;
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
		resetsAt: l.resetsAt ?? (l.resets ? parseResetTime(l.resets, now) : null),
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
			resetsAt: event.resetsAt,
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
				resetsAt: l.resetsAt,
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

/** Parse the CLI's wall clock in its named timezone, never in the phone's timezone. */
export function parseResetTime(text: string, now: Date = new Date()): number | null {
	const match = /^([A-Za-z]{3}) (\d{1,2}) at (\d{1,2})(?::(\d{2}))?\s*(am|pm) \(([^)]+)\)$/i.exec(
		text.trim()
	);
	if (!match) return null;
	const [, monthName, dayText, hourText, minuteText, period, zone] = match;
	const month = [
		'jan',
		'feb',
		'mar',
		'apr',
		'may',
		'jun',
		'jul',
		'aug',
		'sep',
		'oct',
		'nov',
		'dec'
	].indexOf(monthName.toLowerCase());
	const day = Number(dayText),
		hour = Number(hourText),
		minute = Number(minuteText ?? 0);
	if (month < 0 || day < 1 || day > 31 || hour < 1 || hour > 12 || minute > 59) return null;
	try {
		const formatter = new Intl.DateTimeFormat('en-US', {
			timeZone: zone,
			year: 'numeric',
			month: 'numeric',
			day: 'numeric',
			hour: 'numeric',
			minute: 'numeric',
			hourCycle: 'h23'
		});
		const wall = (time: number) => {
			const parts = Object.fromEntries(formatter.formatToParts(time).map((p) => [p.type, p.value]));
			return Date.UTC(
				Number(parts.year),
				Number(parts.month) - 1,
				Number(parts.day),
				Number(parts.hour),
				Number(parts.minute)
			);
		};
		const year = new Date(wall(now.getTime())).getUTCFullYear();
		const candidates: number[] = [];
		for (const y of [year - 1, year, year + 1]) {
			const target = Date.UTC(
				y,
				month,
				day,
				(hour % 12) + (period.toLowerCase() === 'pm' ? 12 : 0),
				minute
			);
			if (new Date(target).getUTCDate() !== day) continue;
			let guess = target;
			for (let i = 0; i < 3; i++) guess += target - wall(guess);
			if (wall(guess) === target) candidates.push(guess);
		}
		candidates.sort((a, b) => Math.abs(a - now.getTime()) - Math.abs(b - now.getTime()));
		return candidates.length ? candidates[0] / 1000 : null;
	} catch {
		return null;
	}
}

export function resetCountdown(resetsAt: number | null, now = Date.now()): string | null {
	if (resetsAt === null || !Number.isFinite(resetsAt)) return null;
	const minutes = Math.ceil((resetsAt * 1000 - now) / 60_000);
	if (minutes <= 0) return 'Reset due';
	const days = Math.floor(minutes / 1440),
		hours = Math.floor((minutes % 1440) / 60),
		mins = minutes % 60;
	const parts = [days ? `${days}d` : '', hours ? `${hours}h` : '', mins ? `${mins}m` : ''].filter(
		Boolean
	);
	return `Resets in ${parts.join(' ')}`;
}

export function contextUsage(
	meta: TranscriptMeta | null
): { used: number; limit: number; percent: number } | null {
	if (!meta) return null;
	const used = Math.max(0, meta.contextTokens ?? 0);
	const limit = meta.contextWindow ?? (meta.model?.endsWith('[1m]') ? 1_000_000 : 200_000);
	if (!Number.isFinite(limit) || limit <= 0 || !Number.isFinite(used)) return null;
	return { used, limit, percent: Math.min(100, (used / limit) * 100) };
}
