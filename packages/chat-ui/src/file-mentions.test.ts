import { describe, expect, it } from 'vitest';
import { insertMention, matchFiles, mentionQuery } from './file-mentions';

describe('file mentions', () => {
	it('finds the @word ending at the caret', () => {
		expect(mentionQuery('@', 1)).toEqual({ start: 0, query: '' });
		expect(mentionQuery('look at @src/ma', 15)).toEqual({ start: 8, query: 'src/ma' });
		expect(mentionQuery('look at @src/ma and', 15)).toEqual({ start: 8, query: 'src/ma' });
		expect(mentionQuery('mail me@example.com', 19)).toBeNull();
		expect(mentionQuery('@src/main.rs done', 17)).toBeNull();
		expect(mentionQuery('no mention', 10)).toBeNull();
	});

	it('ranks file-name matches over path matches over scattered letters', () => {
		const paths = [
			'packages/chat-ui/src/ChatComposer.svelte',
			'docs/composer-notes.md',
			'composer/README.md',
			'src/main.rs',
			'apps/desktop/src/lib/chat-platform.ts'
		];
		expect(matchFiles(paths, 'composer')).toEqual([
			'docs/composer-notes.md',
			'packages/chat-ui/src/ChatComposer.svelte',
			'composer/README.md'
		]);
		expect(matchFiles(paths, 'chcomp')).toEqual(['packages/chat-ui/src/ChatComposer.svelte']);
		expect(matchFiles(paths, '')).toEqual(paths);
		expect(matchFiles(paths, '', 2)).toHaveLength(2);
		expect(matchFiles(paths, 'zzz')).toEqual([]);
	});

	it('replaces the typed mention with the path and moves the caret past it', () => {
		const draft = 'see @src/ma and fix';
		const mention = mentionQuery(draft, 11)!;
		expect(insertMention(draft, mention, 11, 'src/main.rs')).toEqual({
			text: 'see @src/main.rs and fix',
			caret: 17
		});
		expect(insertMention('@', { start: 0, query: '' }, 1, 'my dir/a b.txt')).toEqual({
			text: '@"my dir/a b.txt" ',
			caret: 18
		});
		// The caret was mid-word: the rest of the word goes with the mention.
		expect(insertMention('@sr x', { start: 0, query: 's' }, 2, 'src')).toEqual({
			text: '@src x',
			caret: 5
		});
	});
});
