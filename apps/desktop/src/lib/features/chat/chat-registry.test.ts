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
		agent: string;
		dispose = vi.fn();
		open = vi.fn(async () => {});
		constructor(body: { sessionId?: string; agent?: string }) {
			this.sessionId = body.sessionId ?? '';
			this.agent = body.agent ?? 'claude';
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

	it('keeps a new Codex chat while it has no id, and once the pane stores the one it got', () => {
		const codex = { agent: 'codex' as const, projectPath: '/repo', paneId: 'p1' };
		const first = acquireChat('p1', codex).chat;
		expect(acquireChat('p1', codex).chat).toBe(first);
		first.sessionId = 'thread-1';
		expect(acquireChat('p1', { ...codex, sessionId: 'thread-1' }).chat).toBe(first);
		expect(created).toHaveLength(1);
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
