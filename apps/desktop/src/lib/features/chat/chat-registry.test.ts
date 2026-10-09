import { afterEach, describe, expect, it, vi } from 'vitest';

const created: {
	sessionId: string;
	body: unknown;
	status: string;
	dispose: ReturnType<typeof vi.fn>;
	attach: ReturnType<typeof vi.fn>;
}[] = [];
vi.mock('./agent-api', () => ({ loopbackAgentApi: {} }));
vi.mock('@workbench/chat-ui', () => ({
	AgentChat: class {
		sessionId: string;
		agent: string;
		draft: unknown;
		body: unknown;
		status = 'live';
		dispose = vi.fn();
		attach = vi.fn(async () => {});
		constructor(
			body: { sessionId?: string; agent?: string },
			_api: unknown,
			opts: { draft: unknown }
		) {
			this.body = body;
			this.sessionId = body.sessionId ?? '';
			this.agent = body.agent ?? 'claude';
			this.draft = opts.draft;
			created.push(this);
		}
	},
	ChatDraft: class {
		text = '';
		images: unknown[] = [];
		files: unknown[] = [];
	}
}));

const { acquireChat, followPane, releaseChat } = await import('./chat-registry');
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

	it('follows a /clear re-key: the chat moved to the id its pane now shows', () => {
		const { chat } = acquireChat('p1', body('s1'));
		chat.sessionId = 's1b';
		expect(acquireChat('p1', body('s1b')).chat).toBe(chat);
	});

	it("keeps the pane's draft when its chat is released, and drops it once empty", () => {
		const draft = acquireChat('p1', body('s1')).chat.draft;
		draft.text = 'half a thought';
		releaseChat('p1');
		expect(acquireChat('p1', body('s1')).chat.draft).toBe(draft);
		draft.text = '';
		releaseChat('p1');
		expect(acquireChat('p1', body('s1')).chat.draft).not.toBe(draft);
	});

	it('re-attaches when the pane runs a new spawn, even one this window never saw stop', () => {
		acquireChat('p1', body('s1'), { status: 'running', generation: 1 });
		followPane('p1', { status: 'running', generation: 1 });
		expect(created[0].attach).not.toHaveBeenCalled();

		followPane('p1', { status: 'starting', generation: 2 });
		expect(created[0].attach).not.toHaveBeenCalled();
		followPane('p1', { status: 'running', generation: 2 });
		expect(created[0].attach).toHaveBeenCalledOnce();
		followPane('p1', { status: 'running', generation: 2 });
		expect(created[0].attach).toHaveBeenCalledOnce();
	});

	it('re-attaches an ended chat once its pane runs again', () => {
		acquireChat('p1', body('s1'), { status: 'running', generation: 1 });
		created[0].status = 'exited';
		followPane('p1', { status: 'exited', generation: 1 });
		expect(created[0].attach).not.toHaveBeenCalled();
		// Remounting the view after the restart finds the same chat, ended.
		acquireChat('p1', body('s1'), { status: 'running', generation: 1 });
		expect(created[0].attach).toHaveBeenCalledOnce();
	});

	it('disposes on release', () => {
		acquireChat('p1', body('s1'));
		releaseChat('p1');
		expect(created[0].dispose).toHaveBeenCalled();
	});
});
