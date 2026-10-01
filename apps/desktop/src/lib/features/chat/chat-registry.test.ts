import { afterEach, describe, expect, it, vi } from 'vitest';

const created: {
	sessionId: string;
	dispose: ReturnType<typeof vi.fn>;
	open: ReturnType<typeof vi.fn>;
}[] = [];
vi.mock('./agent-api', () => ({ loopbackAgentApi: {} }));
vi.mock('@workbench/chat-ui', () => ({
	AgentChat: class {
		sessionId: string;
		dispose = vi.fn();
		open = vi.fn(async () => {});
		constructor(body: { sessionId: string }) {
			this.sessionId = body.sessionId;
			created.push(this);
		}
	}
}));

const { acquireChat, isChatClaimed, releaseChat, reopenChat } = await import('./chat-registry');
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

	it('claims the sessions its live chats hold, following a /clear re-key', () => {
		const { chat } = acquireChat('p1', body('claim-1'));
		chat.sessionId = 'claim-1b';
		expect(isChatClaimed('claim-1b')).toBe(true);
		releaseChat('p1');
		expect(isChatClaimed('claim-1b')).toBe(false);
		expect(isChatClaimed('phone')).toBe(false);
	});

	it('reopens the pane chat on request', () => {
		acquireChat('p1', body('s1'));
		reopenChat('p1');
		reopenChat('unknown');
		expect(created[0].open).toHaveBeenCalledOnce();
	});
});
