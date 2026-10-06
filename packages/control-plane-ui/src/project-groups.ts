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
