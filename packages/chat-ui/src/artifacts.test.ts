import { describe, expect, it } from 'vitest';
import type { ArtifactInfo, TranscriptMeta } from '@workbench/types';
import {
	artifactFor,
	artifactLink,
	artifactName,
	chatArtifacts,
	isArtifactTool,
	memoryName,
	promptSuggestions
} from './artifacts';

const info = (toolUseId: string, url: string, action: ArtifactInfo['action'], title?: string) =>
	({ toolUseId, url, action, title }) satisfies ArtifactInfo;

const meta = (extra: Partial<TranscriptMeta>): TranscriptMeta => ({
	title: null,
	model: null,
	permissionMode: null,
	contextTokens: null,
	busy: false,
	tasks: [],
	retry: null,
	rateLimit: null,
	models: [],
	modelChoice: null,
	effort: null,
	...extra
});

describe('chatArtifacts', () => {
	it('lists one row per link, most recently touched first', () => {
		const rows = chatArtifacts([
			info('t1', 'https://claude.ai/artifact/a', 'created', 'Plan'),
			info('t2', 'https://claude.ai/artifact/b', 'created', 'Deck'),
			info('t3', 'https://claude.ai/artifact/a', 'updated'),
			info('t4', 'https://claude.ai/artifact/a', 'opened')
		]);
		expect(rows.map((r) => [r.url, r.title, r.action, r.publishes, r.toolUseId])).toEqual([
			['https://claude.ai/artifact/a', 'Plan', 'opened', 2, 't4'],
			['https://claude.ai/artifact/b', 'Deck', 'created', 1, 't2']
		]);
	});

	it('drops links that are not http(s)', () => {
		expect(chatArtifacts([info('t1', 'javascript:alert(1)', 'created')])).toEqual([]);
		expect(chatArtifacts(undefined)).toEqual([]);
	});
});

describe('artifact helpers', () => {
	it('finds the artifact a call touched', () => {
		const m = meta({ artifacts: [info('t1', 'https://claude.ai/artifact/a', 'created')] });
		expect(artifactFor(m, 't1')?.url).toBe('https://claude.ai/artifact/a');
		expect(artifactFor(m, 't2')).toBeNull();
		expect(artifactFor(null, 't1')).toBeNull();
	});

	it('only opens http(s) links', () => {
		expect(artifactLink('https://claude.ai/artifact/a')).toBe('https://claude.ai/artifact/a');
		expect(artifactLink('file:///etc/passwd')).toBeNull();
		expect(artifactLink('not a url')).toBeNull();
	});

	it('names untitled artifacts by their link', () => {
		expect(artifactName({ url: 'https://claude.ai/artifact/abcdef123456' })).toBe(
			'Artifact abcdef12'
		);
		expect(artifactName({ url: 'https://x', title: 'Plan' })).toBe('Plan');
	});

	it('recognises the artifact tools', () => {
		expect(isArtifactTool('Artifact')).toBe(true);
		expect(isArtifactTool('ArtifactComments')).toBe(true);
		expect(isArtifactTool('Read')).toBe(false);
	});

	it('shortens memory paths but keeps URLs', () => {
		expect(memoryName('/home/u/.claude/memory/style.md')).toBe('style.md');
		expect(memoryName('https://example.com/m/1')).toBe('https://example.com/m/1');
	});
});

describe('promptSuggestions', () => {
	const m = meta({ promptSuggestion: 'Run the tests' });

	it('offers the suggestion while idle with an empty composer', () => {
		expect(promptSuggestions(m, true, '')).toEqual(['Run the tests']);
	});

	it('hides it while busy, offline, or once something is typed', () => {
		expect(promptSuggestions({ ...m, busy: true }, true, '')).toEqual([]);
		expect(promptSuggestions(m, false, '')).toEqual([]);
		expect(promptSuggestions(m, true, 'fix')).toEqual([]);
		expect(promptSuggestions(meta({}), true, '')).toEqual([]);
	});
});
