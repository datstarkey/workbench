import { describe, expect, it } from 'vitest';
import { rewindPreview } from './rewind';

describe('rewindPreview', () => {
	it('lists changed files relative to the cwd, capped', () => {
		const p = rewindPreview(
			{
				canRewind: true,
				filesChanged: ['/w/a.txt', '/w/src/b.ts', '/elsewhere/c'],
				insertions: 3,
				deletions: 1
			},
			'/w',
			2
		);
		expect(p.canRestore).toBe(true);
		expect(p.files).toEqual(['a.txt', 'src/b.ts']);
		expect(p.more).toBe(1);
		expect(p.summary).toBe('Restores 3 files (+3 −1) to how they were before this message.');
	});

	it('explains why code cannot be restored', () => {
		const p = rewindPreview({
			canRewind: false,
			error: 'No file checkpoint found for this message.'
		});
		expect(p.canRestore).toBe(false);
		expect(p.summary).toBe("Code can't be restored: No file checkpoint found for this message.");
		expect(rewindPreview({ canRewind: true, filesChanged: [] }).canRestore).toBe(false);
		expect(rewindPreview(null).summary).toMatch(/Couldn't check/);
	});
});
