import type { RewindFiles } from '@workbench/types';
import { shortPath } from './chat-format';

export interface RewindPreview {
	/** A code restore would change something. */
	canRestore: boolean;
	/** Files it would change, relative to the cwd, at most `max`. */
	files: string[];
	/** How many more files there are past `files`. */
	more: number;
	/** One line on what restoring code does (or why it can't). */
	summary: string;
}

/** What a file rewind's dry run means for the person choosing what to rewind. */
export function rewindPreview(files: RewindFiles | null, cwd?: string, max = 6): RewindPreview {
	const changed = files?.canRewind ? (files.filesChanged ?? []) : [];
	const shown = changed.slice(0, max).map((f) => shortPath(f, cwd));
	let summary: string;
	if (!files) summary = "Couldn't check which files changed.";
	else if (!files.canRewind)
		summary = `Code can't be restored: ${files.error ?? 'no checkpoint for this message'}.`;
	else if (changed.length === 0) summary = 'No file changes to undo since this message.';
	else {
		const count = changed.length === 1 ? '1 file' : `${changed.length} files`;
		summary = `Restores ${count} (+${files.insertions ?? 0} −${files.deletions ?? 0}) to how they were before this message.`;
	}
	return {
		canRestore: changed.length > 0,
		files: shown,
		more: changed.length - shown.length,
		summary: summary.replace(/\.\.$/, '.')
	};
}
