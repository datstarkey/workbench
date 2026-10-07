import type { ArtifactInfo, TranscriptMeta } from '@workbench/types';
import { safeExternalUrl } from './url';

/** One artifact this chat touched, however many calls published to it. */
export interface ChatArtifact {
	url: string;
	title: string | null;
	/** What the newest call did. */
	action: ArtifactInfo['action'];
	/** Calls that published to it (created, updated or published). */
	publishes: number;
	toolUseId: string;
}

/** The tool that publishes and opens artifacts (not its comment/data siblings). */
export function isArtifactTool(name: string): boolean {
	return name === 'Artifact';
}

/** The artifact a tool call published or opened, if it returned a link. */
export function artifactFor(meta: TranscriptMeta | null, toolId: string): ArtifactInfo | null {
	const found = meta?.artifacts?.find((a) => a.toolUseId === toolId);
	return found && safeExternalUrl(found.url) ? found : null;
}

/** The chat's artifacts, most recently touched first, one row per link. */
export function chatArtifacts(list: ArtifactInfo[] | undefined): ChatArtifact[] {
	const byUrl = new Map<string, ChatArtifact>();
	for (const info of list ?? []) {
		const url = safeExternalUrl(info.url);
		if (!url) continue;
		const prev = byUrl.get(url);
		byUrl.delete(url);
		byUrl.set(url, {
			url,
			title: info.title ?? prev?.title ?? null,
			action: info.action,
			publishes: (prev?.publishes ?? 0) + (info.action === 'opened' ? 0 : 1),
			toolUseId: info.toolUseId
		});
	}
	return [...byUrl.values()].reverse();
}

const ACTION_LABELS: Record<ArtifactInfo['action'], string> = {
	created: 'Created',
	updated: 'Updated',
	opened: 'Opened',
	published: 'Published'
};

export function artifactActionLabel(action: ArtifactInfo['action']): string {
	return ACTION_LABELS[action] ?? 'Published';
}

/** A name for an artifact with no title: the end of its link. */
export function artifactName(artifact: { url: string; title?: string | null }): string {
	if (artifact.title) return artifact.title;
	const tail = artifact.url.replace(/\/+$/, '').split('/').pop();
	return tail ? `Artifact ${tail.slice(0, 8)}` : 'Artifact';
}

/** The suggestion chip to show, if any: only while idle and the composer is empty. */
export function promptSuggestions(
	meta: TranscriptMeta | null,
	live: boolean,
	draft: string
): string[] {
	const suggestion = meta?.promptSuggestion?.trim();
	if (!suggestion || !live || meta?.busy || draft.trim()) return [];
	return [suggestion];
}

/** The file name of a recalled memory; organisation memories are URLs and stay whole. */
export function memoryName(path: string): string {
	if (/^https?:/i.test(path)) return path;
	return path.split(/[\\/]/).pop() || path;
}
