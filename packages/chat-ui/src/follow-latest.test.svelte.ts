// @vitest-environment jsdom
import { flushSync } from 'svelte';
import { describe, expect, it } from 'vitest';
import type { TranscriptItem } from '@workbench/types';
import type { PendingPrompt } from './agent-chat.svelte';
import type { Activity } from './chat-format';
import { followLatest } from './follow-latest';

function fakeChat() {
	const chat = $state({
		items: [] as TranscriptItem[],
		pending: [] as PendingPrompt[],
		now: { kind: 'idle' } as Activity
	});
	return chat;
}

/** A scroll box 100px tall over `content` px. */
function scrollBox(content: number) {
	const node = new EventTarget() as EventTarget & Record<string, number>;
	Object.assign(node, { scrollHeight: content, clientHeight: 100, scrollTop: 0 });
	return node as unknown as HTMLElement;
}

const text = (id: string): TranscriptItem => ({ kind: 'text', id, text: id });
const prompt = (id: string) => ({ id }) as PendingPrompt;

describe('followLatest', () => {
	it('follows new items until the reader scrolls up, and sending brings it back', () => {
		const chat = fakeChat();
		const node = scrollBox(1000);
		const cleanup = $effect.root(() => {
			followLatest(chat)(node);
		});
		flushSync();
		expect(node.scrollTop).toBe(1000);

		node.scrollTop = 200;
		node.dispatchEvent(new Event('scroll'));
		chat.items = [text('a')];
		flushSync();
		expect(node.scrollTop).toBe(200);

		chat.pending = [prompt('p')];
		flushSync();
		expect(node.scrollTop).toBe(1000);
		cleanup();
	});

	it('scrolls the viewport the host picks', () => {
		const chat = fakeChat();
		const host = scrollBox(0);
		const viewport = scrollBox(500);
		const cleanup = $effect.root(() => {
			followLatest(chat, () => viewport)(host);
		});
		flushSync();
		expect(viewport.scrollTop).toBe(500);
		cleanup();
	});
});
