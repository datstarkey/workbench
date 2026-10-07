import { expect, it } from 'vitest';
import { matchParts } from './match-parts';

it('splits text around every case-insensitive match', () => {
	const marked = (text: string, q: string) =>
		matchParts(text, q)
			.map((x) => (x.match ? `[${x.text}]` : x.text))
			.join('');
	expect(marked('Workbench', ' bench ')).toBe('Work[bench]');
	expect(marked('aAbaa', 'a')).toBe('[a][A]b[a][a]');
	expect(marked('api', '')).toBe('api');
	expect(marked('api', 'zzz')).toBe('api');
	expect(marked('İstanbul-app', 'app')).toBe('İstanbul-app');
});
