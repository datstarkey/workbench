import { afterEach, describe, expect, it, vi } from 'vitest';

const created: { sessionId: string; dispose: ReturnType<typeof vi.fn> }[] = [];
vi.mock('./agent-api', () => ({ loopbackAgentApi: {} }));
vi.mock('@workbench/chat-ui', () => ({
	AgentChat: class {
		sessionId: string;
		dispose = vi.fn();
		constructor(body: { sessionId: string }) {
			this.sessionId = body.sessionId;
			created.push(this);
		}
	}
}));

const { acquireChat, isChatClaimed, releaseChat } = await import('./chat-registry');
const body = (sessionId: string) => ({ projectPath: '/repo', sessionId, paneId: 'p1' });

describe('chat registry', () => {
	afterEach(() => {
		releaseChat('p1');
		created.length = 0;
	});

	it('keeps one chat per pane across remounts', () => {
		const first = acquireChat('p1', body('s1'));
		const again = acquireChat('p1', body('s1'));
		expect(first.created).toBe(true);
		expect(again.created).toBe(false);
		expect(again.chat).toBe(first.chat);
	});

	it('replaces the chat when the pane moves to another conversation', () => {
		const first = acquireChat('p1', body('s1')).chat;
		const next = acquireChat('p1', body('s2')).chat;
		expect(next).not.toBe(first);
		expect(created[0].dispose).toHaveBeenCalled();
	});

	it('disposes on release', () => {
		acquireChat('p1', body('s1'));
		releaseChat('p1');
		expect(created[0].dispose).toHaveBeenCalled();
	});

	it('claims every session it started, a /clear re-key and released ones included', () => {
		const { chat } = acquireChat('p1', body('claim-1'));
		chat.sessionId = 'claim-1b';
		expect(isChatClaimed('claim-1b')).toBe(true);
		releaseChat('p1');
		expect(isChatClaimed('claim-1')).toBe(true);
		expect(isChatClaimed('phone')).toBe(false);
	});
});
