/** Shared status letter and color mappings for git file statuses */
export const STATUS_DISPLAY: Record<string, { letter: string; color: string }> = {
	modified: { letter: 'M', color: 'text-yellow-400' },
	added: { letter: 'A', color: 'text-green-400' },
	deleted: { letter: 'D', color: 'text-red-400' },
	renamed: { letter: 'R', color: 'text-blue-400' },
	copied: { letter: 'C', color: 'text-blue-400' },
	untracked: { letter: '?', color: 'text-muted-foreground' }
};

const FALLBACK = { letter: '?', color: 'text-muted-foreground' };

export function getStatusDisplay(status: string): { letter: string; color: string } {
	return STATUS_DISPLAY[status] ?? FALLBACK;
}
