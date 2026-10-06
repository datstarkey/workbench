import { expect, it } from 'vitest';
import { matchParts, projectSections } from './project-sections';

const p = (name: string, group?: string) => ({ name, path: `/src/${name}`, group });
const projects = [p('api'), p('site', 'Websites'), p('dots', 'Config'), p('blog', 'Websites')];
const titles = (s: ReturnType<typeof projectSections>) =>
	s.map((x) => `${x.title}: ${x.projects.map((q) => q.name).join(',')}`);

it('puts favourites first, then groups, then ungrouped projects', () => {
	expect(titles(projectSections(projects, new Set(['/src/blog'])))).toEqual([
		'Favourites: blog',
		'Websites: site',
		'Config: dots',
		'Other: api'
	]);
});

it('calls ungrouped projects "Projects" when nothing else is listed', () => {
	expect(titles(projectSections([p('api'), p('cli')], new Set()))).toEqual(['Projects: api,cli']);
});

it('matches the query against name, path and group', () => {
	expect(titles(projectSections(projects, new Set(), 'web'))).toEqual(['Websites: site,blog']);
	expect(titles(projectSections(projects, new Set(), 'src/do'))).toEqual(['Config: dots']);
	expect(projectSections(projects, new Set(), 'nothing')).toEqual([]);
});

it('splits text around every case-insensitive match', () => {
	const marked = (text: string, q: string) =>
		matchParts(text, q)
			.map((x) => (x.match ? `[${x.text}]` : x.text))
			.join('');
	expect(marked('Workbench', ' bench ')).toBe('Work[bench]');
	expect(marked('aAbaa', 'a')).toBe('[a][A]b[a][a]');
	expect(marked('api', '')).toBe('api');
	expect(marked('api', 'zzz')).toBe('api');
});
