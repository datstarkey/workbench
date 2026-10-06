import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import {
	invokeSpy,
	mockInvoke,
	clearInvokeMocks,
	emitMockEvent,
	clearListeners,
	listenSpy
} from '../../test/tauri-mocks';
import { ClaudeSessionStore, type AttentionTarget } from './claudeSessions.svelte';
import type { IntegrationApprovalStore } from './integration-approval.svelte';
import type { WorkspaceStore } from './workspaces.svelte';
import type { ProjectStore } from './projects.svelte';
import type {
	AgentAction,
	AgentAttention,
	AgentSummary,
	DiscoveredClaudeSession
} from '$types/workbench';

function createMockWorkspaceStore(workspaces: unknown[] = []) {
	return {
		workspaces,
		addAISession: vi.fn(() => ({ tabId: 'tab-1' })),
		addAIByProject: vi.fn(),
		updateAISessionByPaneId: vi.fn(),
		updateAITabLabelByPaneId: vi.fn(),
		findAIPaneContext: vi.fn(),
		isChatPane: vi.fn(() => false),
		paneForAgent: vi.fn((): string | null => null)
	} as unknown as WorkspaceStore;
}

function createMockProjectStore() {
	return {
		openProject: vi.fn()
	} as unknown as ProjectStore;
}

function createMockIntegrationApprovalStore() {
	return {
		ensureIntegration: vi.fn(() => Promise.resolve(true))
	} as unknown as IntegrationApprovalStore;
}

describe('ClaudeSessionStore', () => {
	let store: ClaudeSessionStore;
	let mockWorkspaceStore: WorkspaceStore;
	let mockProjectStore: ProjectStore;
	let mockIntegrationApprovalStore: IntegrationApprovalStore;

	beforeEach(() => {
		mockWorkspaceStore = createMockWorkspaceStore();
		mockProjectStore = createMockProjectStore();
		mockIntegrationApprovalStore = createMockIntegrationApprovalStore();
		store = new ClaudeSessionStore(
			mockWorkspaceStore,
			mockProjectStore,
			mockIntegrationApprovalStore
		);
	});

	afterEach(() => {
		clearInvokeMocks();
		clearListeners();
	});

	describe('constructor', () => {
		it('registers 4 event listeners', () => {
			expect(listenSpy).toHaveBeenCalledTimes(4);
		});

		it('registers an agent:attention listener', () => {
			expect(listenSpy).toHaveBeenCalledWith('agent:attention', expect.any(Function));
		});

		it('registers a codex:notify listener', () => {
			expect(listenSpy).toHaveBeenCalledWith('codex:notify', expect.any(Function));
		});

		it('registers a terminal:data listener', () => {
			expect(listenSpy).toHaveBeenCalledWith('terminal:data', expect.any(Function));
		});

		it('registers a terminal:activity listener', () => {
			expect(listenSpy).toHaveBeenCalledWith('terminal:activity', expect.any(Function));
		});
	});

	describe('discoverSessions', () => {
		it('returns sessions and sets discoveredSessions state', async () => {
			const sessions: DiscoveredClaudeSession[] = [
				{ sessionId: 'sess-1', label: 'First session', timestamp: '2025-01-01T00:00:00Z' },
				{ sessionId: 'sess-2', label: 'Second session', timestamp: '2025-01-02T00:00:00Z' }
			];
			mockInvoke('discover_claude_sessions', () => sessions);

			const result = await store.discoverSessions('/projects/test');

			expect(invokeSpy).toHaveBeenCalledWith('discover_claude_sessions', {
				projectPath: '/projects/test'
			});
			expect(result).toEqual(sessions);
			expect(store.discoveredSessions).toEqual(sessions);
		});

		it('handles invoke failure gracefully', async () => {
			const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
			mockInvoke('discover_claude_sessions', () => {
				throw new Error('discovery failed');
			});

			const result = await store.discoverSessions('/projects/test');

			expect(result).toEqual([]);
			expect(errorSpy).toHaveBeenCalledWith(
				'[ClaudeSessionStore] Failed to discover claude sessions:',
				expect.any(Error)
			);
			errorSpy.mockRestore();
		});
	});

	describe('removeDiscoveredSession', () => {
		it('filters claude sessions by default', () => {
			store.discoveredSessions = [
				{ sessionId: 'sess-1', label: 'First', timestamp: '2025-01-01T00:00:00Z' },
				{ sessionId: 'sess-2', label: 'Second', timestamp: '2025-01-02T00:00:00Z' }
			];

			store.removeDiscoveredSession('sess-1');

			expect(store.discoveredSessions).toEqual([
				{ sessionId: 'sess-2', label: 'Second', timestamp: '2025-01-02T00:00:00Z' }
			]);
		});

		it('filters codex sessions when type is codex', () => {
			store.discoveredCodexSessions = [
				{ sessionId: 'codex-1', label: 'Codex 1', timestamp: '2025-01-01T00:00:00Z' },
				{ sessionId: 'codex-2', label: 'Codex 2', timestamp: '2025-01-02T00:00:00Z' }
			];

			store.removeDiscoveredSession('codex-1', 'codex');

			expect(store.discoveredCodexSessions).toEqual([
				{ sessionId: 'codex-2', label: 'Codex 2', timestamp: '2025-01-02T00:00:00Z' }
			]);
		});

		it('does not modify codex sessions when filtering claude sessions', () => {
			store.discoveredSessions = [
				{ sessionId: 'sess-1', label: 'Claude', timestamp: '2025-01-01T00:00:00Z' }
			];
			store.discoveredCodexSessions = [
				{ sessionId: 'codex-1', label: 'Codex', timestamp: '2025-01-01T00:00:00Z' }
			];

			store.removeDiscoveredSession('sess-1');

			expect(store.discoveredSessions).toEqual([]);
			expect(store.discoveredCodexSessions).toEqual([
				{ sessionId: 'codex-1', label: 'Codex', timestamp: '2025-01-01T00:00:00Z' }
			]);
		});
	});

	describe('Claude panes follow agent summaries', () => {
		const summary = (over: Partial<AgentSummary> = {}): AgentSummary => ({
			agent: 'claude',
			sessionId: 'sess-1',
			projectPath: '/test',
			worktreePath: null,
			paneId: 'pane-1',
			claudeAccountId: null,
			title: 'Fix the build',
			model: null,
			busy: false,
			exited: false,
			busySince: null,
			updatedAt: 1,
			waiting: null,
			running: null,
			previousIds: [],
			...over
		});

		function setupClaudePane() {
			(mockWorkspaceStore as { workspaces: unknown[] }).workspaces = [
				{
					id: 'ws-1',
					projectPath: '/test',
					projectName: 'Test',
					terminalTabs: [
						{
							id: 'tab-1',
							label: 'Claude 1',
							split: 'horizontal',
							type: 'claude',
							panes: [{ id: 'pane-1', type: 'claude' }]
						}
					],
					activeTerminalTabId: 'tab-1'
				}
			];
			(mockWorkspaceStore.paneForAgent as ReturnType<typeof vi.fn>).mockReturnValue('pane-1');
		}

		it("labels the tab with the session's title and tracks its turn", () => {
			setupClaudePane();

			store.syncFromAgents([summary({ busy: true })]);
			expect(mockWorkspaceStore.updateAITabLabelByPaneId).toHaveBeenCalledWith(
				'pane-1',
				'Fix the build',
				'claude'
			);
			expect(store.panesInProgress.has('pane-1')).toBe(true);

			store.syncFromAgents([summary({ title: 'Renamed' })]);
			expect(mockWorkspaceStore.updateAITabLabelByPaneId).toHaveBeenLastCalledWith(
				'pane-1',
				'Renamed',
				'claude'
			);
			expect(store.panesInProgress.has('pane-1')).toBe(false);
		});

		it("follows the pane's claude onto another session, newest first", () => {
			setupClaudePane();

			store.syncFromAgents([
				summary({ sessionId: 'old', updatedAt: 1 }),
				summary({ sessionId: 'resumed', updatedAt: 2 })
			]);

			expect(mockWorkspaceStore.updateAISessionByPaneId).toHaveBeenCalledTimes(1);
			expect(mockWorkspaceStore.updateAISessionByPaneId).toHaveBeenCalledWith(
				'pane-1',
				'resumed',
				'claude'
			);
		});

		it("leaves a chat pane's session id to its chat", () => {
			setupClaudePane();
			(mockWorkspaceStore.isChatPane as ReturnType<typeof vi.fn>).mockReturnValue(true);

			store.syncFromAgents([summary({ sessionId: 'new-session-after-clear' })]);

			expect(mockWorkspaceStore.updateAISessionByPaneId).not.toHaveBeenCalled();
		});

		it('is not busy while waiting on someone, nor once its session is gone', () => {
			setupClaudePane();
			const waiting = { id: 'r1', tool: 'Bash', preview: 'ls' };

			store.syncFromAgents([summary({ busy: true, waiting })]);
			expect(store.panesInProgress.has('pane-1')).toBe(false);

			store.syncFromAgents([summary({ busy: true })]);
			expect(store.panesInProgress.has('pane-1')).toBe(true);
			store.syncFromAgents([]);
			expect(store.panesInProgress.has('pane-1')).toBe(false);

			store.syncFromAgents([summary({ busy: true })]);
			store.syncFromAgents([summary({ busy: true, exited: true })]);
			expect(store.panesInProgress.has('pane-1')).toBe(false);
		});

		it('ignores exited sessions, Codex ones and panes that are not Claude', () => {
			setupClaudePane();
			store.syncFromAgents([summary({ exited: true, busy: true })]);
			store.syncFromAgents([summary({ agent: 'codex', busy: true })]);
			expect(store.panesInProgress.has('pane-1')).toBe(false);

			(mockWorkspaceStore.paneForAgent as ReturnType<typeof vi.fn>).mockReturnValue('shell-pane');
			store.syncFromAgents([summary({ busy: true })]);
			expect(store.panesInProgress.has('shell-pane')).toBe(false);
			expect(mockWorkspaceStore.updateAITabLabelByPaneId).not.toHaveBeenCalled();
		});
	});

	describe('Codex label discovery', () => {
		const sessionId = 'abcd1234-5678-9012-3456-789012345678';
		const discoverCalls = () =>
			invokeSpy.mock.calls.filter(([cmd]) => cmd === 'discover_codex_sessions').length;

		/** Wait until `count` discoveries have been issued *and* their continuations ran. */
		async function settleDiscovery(count: number) {
			await vi.waitFor(() => expect(discoverCalls()).toBe(count));
			await new Promise((resolve) => setTimeout(resolve, 0));
		}

		const notify = (turnComplete: boolean) =>
			emitMockEvent('codex:notify', {
				paneId: 'pane-1',
				sessionId,
				...(turnComplete ? { notifyEvent: 'agent-turn-complete' } : {})
			});

		beforeEach(() => {
			(mockWorkspaceStore as { workspaces: unknown[] }).workspaces = [
				{
					id: 'ws-1',
					projectPath: '/test',
					projectName: 'Test',
					terminalTabs: [
						{
							id: 'tab-1',
							label: 'Codex 1',
							split: 'horizontal',
							type: 'codex',
							panes: [{ id: 'pane-1', type: 'codex' }]
						}
					],
					activeTerminalTabId: 'tab-1'
				}
			];
			(mockWorkspaceStore.findAIPaneContext as ReturnType<typeof vi.fn>).mockReturnValue({
				projectPath: '/test',
				cwd: '/test'
			});
		});

		it('recovers the real label once the first user message exists on disk', async () => {
			// The first notify can come before the thread's first message is on disk.
			mockInvoke('discover_codex_sessions', () => [
				{ sessionId, label: 'Session abcd1234', timestamp: '' }
			]);
			notify(false);
			// Let that first discovery fully settle before the next event, so the
			// "no label yet" result is actually recorded.
			await settleDiscovery(1);

			mockInvoke('discover_codex_sessions', () => [
				{ sessionId, label: 'Fix the polling bug', timestamp: '' }
			]);
			notify(true);

			await vi.waitFor(() =>
				expect(mockWorkspaceStore.updateAITabLabelByPaneId).toHaveBeenCalledWith(
					'pane-1',
					'Fix the polling bug',
					'codex'
				)
			);
		});

		it('does not rediscover once a label is resolved', async () => {
			mockInvoke('discover_codex_sessions', () => [
				{ sessionId, label: 'Already named', timestamp: '' }
			]);
			notify(false);
			await settleDiscovery(1);

			for (let i = 0; i < 3; i++) {
				notify(true);
				await new Promise((resolve) => setTimeout(resolve, 0));
			}

			expect(discoverCalls()).toBe(1);
		});

		it('does not rescan on events that cannot have named the thread', async () => {
			mockInvoke('discover_codex_sessions', () => [
				{ sessionId, label: 'Session abcd1234', timestamp: '' }
			]);
			notify(false);
			await settleDiscovery(1);

			for (let i = 0; i < 5; i++) {
				notify(false);
				await new Promise((resolve) => setTimeout(resolve, 0));
			}

			expect(discoverCalls()).toBe(1);
		});

		it('stops retrying discovery after the attempt cap', async () => {
			mockInvoke('discover_codex_sessions', () => [
				{ sessionId, label: 'Session abcd1234', timestamp: '' }
			]);

			for (let i = 0; i < 12; i++) {
				notify(true);
				await new Promise((resolve) => setTimeout(resolve, 0));
			}

			expect(discoverCalls()).toBe(6);
		});
	});

	describe('agent:attention events', () => {
		const attention = (kind: AgentAttention['kind'], over: Partial<AgentAttention> = {}) => ({
			kind,
			agent: 'claude',
			sessionId: 'sess-1',
			previousIds: [],
			paneId: null,
			terminalId: 'term-1',
			projectPath: '/repos/app',
			worktreePath: null,
			title: 'Fix the build',
			waiting: null,
			busy: false,
			...over
		});
		let notified: [AttentionTarget, AgentAttention['kind']][];

		beforeEach(() => {
			notified = [];
			store.onAttention((target, kind) => notified.push([target, kind]));
		});

		it("flags and notifies the session's pane, then clears it", () => {
			(mockWorkspaceStore.paneForAgent as ReturnType<typeof vi.fn>).mockReturnValue('pane-1');
			store.panesInProgress.add('pane-1');

			emitMockEvent('agent:attention', attention('waiting'));
			expect(store.panesAwaitingInput.has('pane-1')).toBe(true);
			expect(store.panesInProgress.has('pane-1')).toBe(false);

			// Answered mid-turn: the turn goes on.
			emitMockEvent('agent:attention', attention('resolved', { busy: true }));
			expect(store.panesAwaitingInput.has('pane-1')).toBe(false);
			expect(store.panesInProgress.has('pane-1')).toBe(true);

			emitMockEvent('agent:attention', attention('turnEnded'));
			expect(store.panesInProgress.has('pane-1')).toBe(false);
			const pane = { paneId: 'pane-1', sessionId: 'sess-1' };
			expect(notified).toEqual([
				[pane, 'waiting'],
				[pane, 'resolved'],
				[pane, 'turnEnded']
			]);
			expect(mockWorkspaceStore.paneForAgent).toHaveBeenCalledWith(
				expect.objectContaining({ sessionId: 'sess-1', terminalId: 'term-1' })
			);
		});

		it('notifies about a session no pane shows, by its title', () => {
			emitMockEvent('agent:attention', attention('waiting'));
			emitMockEvent('agent:attention', attention('resolved'));
			emitMockEvent('agent:attention', attention('turnEnded', { title: null }));
			const phone = { id: 'sess-1', projectPath: '/repos/app' };
			expect(notified).toEqual([
				[{ ...phone, label: 'Fix the build' }, 'waiting'],
				[{ ...phone, label: 'Fix the build' }, 'resolved'],
				[{ ...phone, label: 'Session sess-1' }, 'turnEnded']
			]);
		});
	});

	describe('terminal:activity events for Codex panes', () => {
		function setupCodexPane() {
			(mockWorkspaceStore as { workspaces: unknown[] }).workspaces = [
				{
					id: 'ws-1',
					projectPath: '/test',
					projectName: 'Test',
					terminalTabs: [
						{
							id: 'tab-1',
							label: 'Codex 1',
							split: 'horizontal',
							type: 'codex',
							panes: [{ id: 'pane-1', type: 'codex' }]
						}
					],
					activeTerminalTabId: 'tab-1'
				}
			];
		}

		it('clears panesInProgress on inactive event', () => {
			setupCodexPane();
			store.panesInProgress.add('pane-1');

			emitMockEvent('terminal:activity', { sessionId: 'pane-1', active: false });

			expect(store.panesInProgress.has('pane-1')).toBe(false);
		});

		it('ignores active events', () => {
			setupCodexPane();
			store.panesInProgress.add('pane-1');

			emitMockEvent('terminal:activity', { sessionId: 'pane-1', active: true });

			expect(store.panesInProgress.has('pane-1')).toBe(true);
		});
	});

	describe('startSession', () => {
		it('delegates to workspaces.addAISession', async () => {
			await store.startSession('ws-1', 'claude');
			expect(mockWorkspaceStore.addAISession).toHaveBeenCalledWith('ws-1', 'claude');
		});
	});

	describe('startSessionByProject', () => {
		it('opens project and adds AI session by project', async () => {
			await store.startSessionByProject('/projects/test', 'claude');
			expect(mockProjectStore.openProject).toHaveBeenCalledWith('/projects/test');
			expect(mockWorkspaceStore.addAIByProject).toHaveBeenCalledWith('/projects/test', 'claude');
		});
	});

	describe('startAgentActionByProject', () => {
		it('opens project and starts a labeled action session by project', async () => {
			const action: AgentAction = {
				id: 'action-1',
				name: 'Security Scan',
				prompt: 'Scan this codebase for security issues',
				target: 'both',
				category: 'Security',
				tags: ['security']
			};

			await store.startAgentActionByProject('/projects/test', action, 'codex');

			expect(mockIntegrationApprovalStore.ensureIntegration).toHaveBeenCalledWith('codex');
			expect(mockProjectStore.openProject).toHaveBeenCalledWith('/projects/test');
			expect(mockWorkspaceStore.addAIByProject).toHaveBeenCalledWith('/projects/test', 'codex', {
				label: 'Security Scan',
				prompt: 'Scan this codebase for security issues'
			});
		});

		it('starts nothing when the integration is declined', async () => {
			(
				mockIntegrationApprovalStore.ensureIntegration as ReturnType<typeof vi.fn>
			).mockResolvedValue(false);
			const action = { name: 'Scan', prompt: 'Scan' } as AgentAction;

			await store.startAgentActionByProject('/projects/test', action, 'claude');
			await store.startAgentActionInWorkspace({ id: 'ws-1' }, action, 'claude');

			expect(mockProjectStore.openProject).not.toHaveBeenCalled();
			expect(mockWorkspaceStore.addAIByProject).not.toHaveBeenCalled();
			expect(mockWorkspaceStore.addAISession).not.toHaveBeenCalled();
		});
	});

	describe('startAgentActionInWorkspace', () => {
		it('starts a claude session with action label and prompt', async () => {
			const action: AgentAction = {
				id: 'action-1',
				name: 'Review PR',
				prompt: 'Review this PR for regressions',
				target: 'both',
				category: 'Code Review',
				tags: ['review']
			};

			await store.startAgentActionInWorkspace({ id: 'ws-1' }, action, 'claude');

			expect(mockIntegrationApprovalStore.ensureIntegration).toHaveBeenCalledWith('claude');
			expect(mockWorkspaceStore.addAISession).toHaveBeenCalledWith('ws-1', 'claude', {
				label: 'Review PR',
				prompt: 'Review this PR for regressions'
			});
		});
	});

	describe('syncLabelFromSession (via events)', () => {
		function setupCodexPane() {
			(mockWorkspaceStore as { workspaces: unknown[] }).workspaces = [
				{
					id: 'ws-1',
					projectPath: '/test',
					projectName: 'Test',
					terminalTabs: [
						{
							id: 'tab-1',
							label: 'Codex 1',
							split: 'horizontal',
							type: 'codex',
							panes: [{ id: 'pane-1', type: 'codex' }]
						}
					],
					activeTerminalTabId: 'tab-1'
				}
			];
		}

		it('Codex notify metadata and the shared attention event raise one completion alert', () => {
			setupCodexPane();
			const alert = vi.fn();
			store.onAttention(alert);
			(mockWorkspaceStore.paneForAgent as ReturnType<typeof vi.fn>).mockReturnValue('pane-1');
			emitMockEvent('terminal:data', { sessionId: 'pane-1', data: 'working' });
			emitMockEvent('codex:notify', {
				paneId: 'pane-1',
				sessionId: 'thread',
				notifyEvent: 'agent-turn-complete'
			});
			expect(alert).not.toHaveBeenCalled();
			emitMockEvent('agent:attention', {
				kind: 'turnEnded',
				agent: 'codex',
				sessionId: 'thread',
				paneId: 'pane-1',
				terminalOnly: true
			});
			expect(alert).toHaveBeenCalledTimes(1);
		});

		it('codex:notify with sessionId sets fallback label then resolves to discovered label', async () => {
			setupCodexPane();

			const sessions: DiscoveredClaudeSession[] = [
				{ sessionId: 'codex-sess-1', label: 'Fix auth bug', timestamp: '2025-01-01T00:00:00Z' }
			];
			mockInvoke('discover_codex_sessions', () => sessions);

			(mockWorkspaceStore.findAIPaneContext as ReturnType<typeof vi.fn>).mockReturnValue({
				projectPath: '/test',
				cwd: '/test'
			});

			emitMockEvent('codex:notify', {
				paneId: 'pane-1',
				sessionId: 'codex-sess-1'
			});

			// Fallback label set synchronously
			expect(mockWorkspaceStore.updateAITabLabelByPaneId).toHaveBeenCalledWith(
				'pane-1',
				'Session codex-se',
				'codex'
			);

			// Wait for async discover to complete
			await vi.waitFor(() => {
				expect(mockWorkspaceStore.updateAITabLabelByPaneId).toHaveBeenCalledWith(
					'pane-1',
					'Fix auth bug',
					'codex'
				);
			});
		});

		it("codex:notify leaves a chat pane's thread id to its chat", () => {
			setupCodexPane();
			(mockWorkspaceStore.isChatPane as ReturnType<typeof vi.fn>).mockReturnValue(true);
			emitMockEvent('codex:notify', { paneId: 'pane-1', sessionId: 'codex-sess-2' });
			expect(mockWorkspaceStore.updateAISessionByPaneId).not.toHaveBeenCalled();
		});

		it('stale-session guard: does not update label if session changed before discover returns', async () => {
			setupCodexPane();

			let resolveDiscover!: (sessions: DiscoveredClaudeSession[]) => void;
			mockInvoke(
				'discover_codex_sessions',
				() =>
					new Promise<DiscoveredClaudeSession[]>((r) => {
						resolveDiscover = r;
					})
			);

			(mockWorkspaceStore.findAIPaneContext as ReturnType<typeof vi.fn>).mockReturnValue({
				projectPath: '/test',
				cwd: '/test'
			});

			// First session event
			emitMockEvent('codex:notify', {
				paneId: 'pane-1',
				sessionId: 'session-old'
			});

			// Second session event (supersedes the first)
			emitMockEvent('codex:notify', {
				paneId: 'pane-1',
				sessionId: 'session-new'
			});

			// Now resolve the first discover call — should be ignored because latest is session-new
			resolveDiscover([
				{ sessionId: 'session-old', label: 'Stale label', timestamp: '2025-01-01T00:00:00Z' }
			]);
			await new Promise((r) => setTimeout(r, 0));

			// The label should NOT have been updated with the stale session's label
			const labelCalls = (mockWorkspaceStore.updateAITabLabelByPaneId as ReturnType<typeof vi.fn>)
				.mock.calls;
			const resolvedLabels = labelCalls
				.filter((c: unknown[]) => !String(c[1]).startsWith('Session '))
				.map((c: unknown[]) => c[1]);
			expect(resolvedLabels).not.toContain('Stale label');
		});

		it('keeps fallback label when discover returns no matching session', async () => {
			setupCodexPane();

			mockInvoke('discover_codex_sessions', () => [
				{
					sessionId: 'other-session',
					label: 'Other session',
					timestamp: '2025-01-01T00:00:00Z'
				}
			]);

			(mockWorkspaceStore.findAIPaneContext as ReturnType<typeof vi.fn>).mockReturnValue({
				projectPath: '/test',
				cwd: '/test'
			});

			emitMockEvent('codex:notify', {
				paneId: 'pane-1',
				sessionId: 'my-session'
			});

			await new Promise((r) => setTimeout(r, 0));

			// Only the fallback label should have been set, not a resolved one
			const labelCalls = (mockWorkspaceStore.updateAITabLabelByPaneId as ReturnType<typeof vi.fn>)
				.mock.calls;
			const labels = labelCalls.map((c: unknown[]) => c[1]);
			expect(labels).toContain('Session my-sessi');
			expect(labels).not.toContain('Other session');
		});
	});

	describe('discoverCodexSessions', () => {
		it('returns sessions and sets discoveredCodexSessions state', async () => {
			const sessions: DiscoveredClaudeSession[] = [
				{ sessionId: 'codex-1', label: 'Codex session', timestamp: '2025-01-01T00:00:00Z' }
			];
			mockInvoke('discover_codex_sessions', () => sessions);

			const result = await store.discoverCodexSessions('/projects/test');

			expect(invokeSpy).toHaveBeenCalledWith('discover_codex_sessions', {
				projectPath: '/projects/test'
			});
			expect(result).toEqual(sessions);
			expect(store.discoveredCodexSessions).toEqual(sessions);
		});

		it('handles invoke failure gracefully', async () => {
			const errorSpy = vi.spyOn(console, 'error').mockImplementation(() => {});
			mockInvoke('discover_codex_sessions', () => {
				throw new Error('codex discovery failed');
			});

			const result = await store.discoverCodexSessions('/projects/test');

			expect(result).toEqual([]);
			expect(errorSpy).toHaveBeenCalledWith(
				'[ClaudeSessionStore] Failed to discover codex sessions:',
				expect.any(Error)
			);
			errorSpy.mockRestore();
		});
	});
});
