import { afterEach, describe, expect, it, vi } from 'vitest';
import type { AgentChat } from './agent-chat.svelte';
import { CodexControlsStore } from './codex-controls.svelte';
describe('CodexControlsStore', () => {
	afterEach(() => vi.unstubAllGlobals());
	it('opens the fork as a separate conversation and leaves the original id alone', async () => {
		const chat = {
			sessionId: 'original',
			codexAction: vi.fn().mockResolvedValue({ thread: { id: 'fork', name: 'Fork' } })
		} as unknown as AgentChat;
		const onThread = vi.fn();
		const store = new CodexControlsStore(chat, onThread);
		await store.fork();
		expect(onThread).toHaveBeenCalledWith('fork', 'Fork');
		expect(chat.sessionId).toBe('original');
	});
	it('surfaces unsupported features and releases the pending action state', async () => {
		const chat = {
			codexAction: vi.fn().mockRejectedValue(new Error('Unsupported by this Codex'))
		} as unknown as AgentChat;
		const store = new CodexControlsStore(chat, vi.fn());
		expect(await store.run('remoteEnable')).toBeNull();
		expect(store.error).toBe('Unsupported by this Codex');
		expect(store.busy).toBe(false);
	});
	it('closes a microphone acquired after its pane was disposed', async () => {
		let acquired!: (v: MediaStream) => void;
		const stop = vi.fn();
		const codexAction = vi.fn();
		vi.stubGlobal('navigator', {
			mediaDevices: {
				getUserMedia: vi.fn(() => new Promise<MediaStream>((resolve) => (acquired = resolve)))
			}
		});
		const store = new CodexControlsStore({ codexAction } as unknown as AgentChat, vi.fn());
		const starting = store.startVoice();
		store.dispose();
		acquired({ getTracks: () => [{ stop }] } as unknown as MediaStream);
		await starting;
		expect(stop).toHaveBeenCalledOnce();
		expect(codexAction).not.toHaveBeenCalled();
		expect(store.mic).toBe(false);
	});
	it('paginates provider history without changing the cwd/search filter', async () => {
		const codexAction = vi
			.fn()
			.mockResolvedValueOnce({ data: [{ id: 'a', name: 'A' }], nextCursor: 'next' })
			.mockResolvedValueOnce({ data: [{ id: 'b', name: 'B' }], nextCursor: null });
		const store = new CodexControlsStore({ codexAction } as unknown as AgentChat, vi.fn());
		store.search = 'changes';
		store.archived = true;
		await store.list();
		await store.list(true);
		expect(store.threads.map((t) => t.id)).toEqual(['a', 'b']);
		expect(codexAction).toHaveBeenLastCalledWith('threads', {
			search: 'changes',
			archived: true,
			cursor: 'next'
		});
	});
});
