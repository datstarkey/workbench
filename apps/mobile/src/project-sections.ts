import { groupProjects } from '@workbench/control-plane-ui';
import type { ProjectConfig } from '@workbench/types';

export interface ProjectSection {
	/** Stable id, used to remember a collapsed section. */
	key: string;
	title: string;
	kind: 'favourites' | 'group' | 'other';
	projects: ProjectConfig[];
}

/**
 * Favourites first, then the desktop's project groups, then ungrouped projects.
 * A favourite shows only under Favourites. The query matches name, path or group.
 */
export function projectSections(
	projects: ProjectConfig[],
	favourites: ReadonlySet<string>,
	query = ''
): ProjectSection[] {
	const q = query.trim().toLowerCase();
	const matches = q
		? projects.filter((p) =>
				[p.name, p.path, p.group ?? ''].some((s) => s.toLowerCase().includes(q))
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

export interface TextPart {
	text: string;
	match: boolean;
}

/** `text` split around every case-insensitive occurrence of `query`, for highlighting. */
export function matchParts(text: string, query: string): TextPart[] {
	const q = query.trim().toLowerCase();
	const lower = text.toLowerCase();
	// Offsets in `lower` only map onto `text` when lowercasing keeps the length.
	if (!q || lower.length !== text.length) return [{ text, match: false }];
	const parts: TextPart[] = [];
	let from = 0;
	for (let at = lower.indexOf(q); at !== -1; at = lower.indexOf(q, from)) {
		if (at > from) parts.push({ text: text.slice(from, at), match: false });
		parts.push({ text: text.slice(at, at + q.length), match: true });
		from = at + q.length;
	}
	if (from < text.length) parts.push({ text: text.slice(from), match: false });
	return parts;
}
