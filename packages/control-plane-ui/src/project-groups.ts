import type { ProjectConfig } from '@workbench/types';

export interface ProjectGroup {
	group: string | null;
	projects: ProjectConfig[];
}

/** Named groups first (in first-appearance order), ungrouped projects last. */
export function groupProjects(projects: ProjectConfig[]): ProjectGroup[] {
	const named = new Map<string, ProjectConfig[]>();
	const ungrouped: ProjectConfig[] = [];
	for (const p of projects) {
		if (!p.group) ungrouped.push(p);
		else if (named.has(p.group)) named.get(p.group)!.push(p);
		else named.set(p.group, [p]);
	}
	const groups: ProjectGroup[] = [...named].map(([group, projects]) => ({ group, projects }));
	if (ungrouped.length > 0) groups.push({ group: null, projects: ungrouped });
	return groups;
}

export interface ProjectSection {
	/** Stable id, used to remember a collapsed section. */
	key: string;
	title: string;
	kind: 'favourites' | 'group' | 'other';
	projects: ProjectConfig[];
}

/**
 * Favourites first, then the desktop's project groups, then ungrouped projects.
 * A favourite shows only under Favourites. The query matches name or group, and the path
 * only once it contains a `/`: every path shares a prefix like `/Users/…`, so a plain
 * word would match them all.
 */
export function projectSections(
	projects: ProjectConfig[],
	favourites: ReadonlySet<string>,
	query = ''
): ProjectSection[] {
	const q = query.trim().toLowerCase();
	const matches = q
		? projects.filter((p) =>
				[p.name, p.group ?? '', q.includes('/') ? p.path : ''].some((s) =>
					s.toLowerCase().includes(q)
				)
			)
		: projects;
	const favs = matches.filter((p) => favourites.has(p.path));
	const sections: ProjectSection[] = favs.length
		? [{ key: 'favourites', title: 'Favourites', kind: 'favourites', projects: favs }]
		: [];
	const groups = groupProjects(matches.filter((p) => !favourites.has(p.path)));
	for (const { group, projects } of groups) {
		sections.push(
			group
				? { key: `group:${group}`, title: group, kind: 'group', projects }
				: {
						key: 'other',
						title: sections.length ? 'Other' : 'Projects',
						kind: 'other',
						projects
					}
		);
	}
	return sections;
}
