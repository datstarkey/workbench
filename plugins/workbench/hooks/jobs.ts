import { taskNotification, taskStatus, type Line } from './lines';

// Background jobs (`run_in_background` Bash) in the tasks panel, by id, with
// the status last reported. A job's own notification says how it ended; the
// Stop hook's job list is the fallback when no notification names it.

const jobs = new Map<string, string>();

/** A job just went to the background; false if it was reported already. */
export function startJob(id: string): boolean {
	if (jobs.has(id)) return false;
	jobs.set(id, 'running');
	return true;
}

function report(id: string, status: string, summary?: string): Line | undefined {
	if (jobs.get(id) === status) return undefined;
	jobs.set(id, status);
	if (status === 'running') return undefined;
	return {
		type: 'system',
		subtype: 'task_notification',
		task_id: id,
		status,
		...(summary ? { summary } : {}),
		uuid: `wbmod-bg-done-${id}`
	};
}

/** A `task-notification` delivery: the real outcome (completed, failed, killed). */
export function notifiedJob(text: string): Line | undefined {
	const note = taskNotification(text);
	return note && jobs.has(note.id) ? report(note.id, note.status, note.summary) : undefined;
}

/**
 * The Stop hook's job list. A running job missing from it has ended; it's
 * called completed until its notification (if one comes) says otherwise.
 */
export function stoppedJobs(tasks: readonly { id: string; status: string }[]): Line[] {
	const live = new Map(tasks.map((t) => [t.id, t.status]));
	return [...jobs]
		.filter(([, status]) => status === 'running')
		.map(([id]) => report(id, taskStatus(live.get(id) ?? 'completed')))
		.filter((line): line is Line => line !== undefined);
}
