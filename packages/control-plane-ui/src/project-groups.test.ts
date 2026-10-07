import { expect, it } from 'vitest';
import { groupProjects, projectSections } from './project-groups';

it('lists named groups in first-appearance order with ungrouped projects last', () => {
	const p = (name: string, group?: string) => ({ name, path: `/${name}`, group });
	expect(
		groupProjects([p('a'), p('b', 'Web'), p('c', 'Config'), p('d', 'Web'), p('e', '')])
	).toEqual([
		{ group: 'Web', projects: [p('b', 'Web'), p('d', 'Web')] },
		{ group: 'Config', projects: [p('c', 'Config')] },
		{ group: null, projects: [p('a'), p('e', '')] }
	]);
	expect(groupProjects([])).toEqual([]);
});

const sp = (name: string, group?: string) => ({ name, path: `/src/${name}`, group });
const projects = [sp('api'), sp('site', 'Websites'), sp('dots', 'Config'), sp('blog', 'Websites')];
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
	expect(titles(projectSections([sp('api'), sp('cli')], new Set()))).toEqual(['Projects: api,cli']);
});

it('matches the query against name, group and, once it has a slash, path', () => {
	expect(titles(projectSections(projects, new Set(), 'web'))).toEqual(['Websites: site,blog']);
	expect(titles(projectSections(projects, new Set(), 'src/do'))).toEqual(['Config: dots']);
	expect(projectSections(projects, new Set(), 'src')).toEqual([]);
	expect(projectSections(projects, new Set(), 'nothing')).toEqual([]);
});
