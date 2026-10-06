import { expect, it } from 'vitest';
import { groupProjects } from './project-groups';

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
