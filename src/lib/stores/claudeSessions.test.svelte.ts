import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import {
	invokeSpy,
	mockInvoke,
	clearInvokeMocks,
	emitMockEvent,
	clearListeners,
	listenSpy
} from '../../test/tauri-mocks';
import { ClaudeSessionStore } from './claudeSessions.svelte';
import type { IntegrationApprovalStore } from './integration-approval.svelte';
import type { WorkspaceStore } from './workspaces.svelte';
import type { ProjectStore } from './projects.svelte';
import type { DiscoveredClaudeSession } from '$types/workbench';

function createMockWorkspaceStore(workspaces: unknown[] = []) {
	return {
		workspaces,
		addAISession: vi.fn(() => ({ tabId: 'tab-1' })),
		addAIByProject: vi.fn(),
		updateAISessionByPaneId: vi.fn(),
		updateAITabLabelByPaneId: vi.fn(),
		findAIPaneContext: vi.fn()
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
		it('registers 3 event listeners', () => {
			expect(listenSpy).toHaveBeenCalledTimes(3);
		});

		it('registers a claude:hook listener', () => {
			expect(listenSpy).toHaveBeenCalledWith('claude:hook', expect.any(Function));
		});

		it('registers a codex:notify listener', () => {
			expect(listenSpy).toHaveBeenCalledWith('codex:notify', expect.any(Function));
		});

		it('registers a terminal:data listener', () => {
			expect(listenSpy).toHaveBeenCalledWith('terminal:data', expect.any(Function));
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
				'[ClaudeSessionStore] Failed to discover sessions:',
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

	describe('claude:hook events', () => {
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
		}

		it('UserPromptSubmit adds pane to panesInProgress', () => {
			setupClaudePane();

			emitMockEvent('claude:hook', {
				paneId: 'pane-1',
				hookEventName: 'UserPromptSubmit',
				hookPayload: {}
			});

			expect(store.panesInProgress.has('pane-1')).toBe(true);
		});

		it('Stop removes pane from panesInProgress', () => {
			setupClaudePane();

			// First add the pane
			emitMockEvent('claude:hook', {
				paneId: 'pane-1',
				hookEventName: 'UserPromptSubmit',
				hookPayload: {}
			});
			expect(store.panesInProgress.has('pane-1')).toBe(true);

			// Then stop it
			emitMockEvent('claude:hook', {
				paneId: 'pane-1',
				hookEventName: 'Stop',
				hookPayload: {}
			});

			expect(store.panesInProgress.has('pane-1')).toBe(false);
		});

		it('SessionStart removes pane from panesInProgress', () => {
			setupClaudePane();

			// Add the pane
			emitMockEvent('claude:hook', {
				paneId: 'pane-1',
				hookEventName: 'UserPromptSubmit',
				hookPayload: {}
			});
			expect(store.panesInProgress.has('pane-1')).toBe(true);

			// SessionStart should remove it
			emitMockEvent('claude:hook', {
				paneId: 'pane-1',
				hookEventName: 'SessionStart',
				hookPayload: {}
			});

			expect(store.panesInProgress.has('pane-1')).toBe(false);
		});

		it('with sessionId updates workspace store', () => {
			setupClaudePane();

			emitMockEvent('claude:hook', {
				paneId: 'pane-1',
				sessionId: 'session-abc-123',
				hookEventName: 'UserPromptSubmit',
				hookPayload: {}
			});

			expect(mockWorkspaceStore.updateAISessionByPaneId).toHaveBeenCalledWith(
				'pane-1',
				'session-abc-123',
				'claude'
			);
		});

		it('with sessionId updates tab label', () => {
			setupClaudePane();

			emitMockEvent('claude:hook', {
				paneId: 'pane-1',
				sessionId: 'abcd1234-5678-9012-3456-789012345678',
				hookEventName: 'UserPromptSubmit',
				hookPayload: {}
			});

			expect(mockWorkspaceStore.updateAITabLabelByPaneId).toHaveBeenCalledWith(
				'pane-1',
				'Session abcd1234',
				'claude'
			);
		});

		it('ignores events for non-claude panes', () => {
			// Set up a shell pane, not claude
			(mockWorkspaceStore as { workspaces: unknown[] }).workspaces = [
				{
					id: 'ws-1',
					projectPath: '/test',
					projectName: 'Test',
					terminalTabs: [
						{
							id: 'tab-1',
							label: 'Shell',
							split: 'horizontal',
							type: 'shell',
							panes: [{ id: 'pane-1', type: 'shell' }]
						}
					],
					activeTerminalTabId: 'tab-1'
				}
			];

			emitMockEvent('claude:hook', {
				paneId: 'pane-1',
				hookEventName: 'UserPromptSubmit',
				hookPayload: {}
			});

			expect(store.panesInProgress.has('pane-1')).toBe(false);
		});

		it('Notification with idle_prompt removes pane from panesInProgress', () => {
			setupClaudePane();

			emitMockEvent('claude:hook', {
				paneId: 'pane-1',
				hookEventName: 'UserPromptSubmit',
				hookPayload: {}
			});
			expect(store.panesInProgress.has('pane-1')).toBe(true);

			emitMockEvent('claude:hook', {
				paneId: 'pane-1',
				hookEventName: 'Notification',
				hookPayload: { notification_type: 'idle_prompt' }
			});

			expect(store.panesInProgress.has('pane-1')).toBe(false);
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
				'[ClaudeSessionStore] Failed to discover Codex sessions:',
				expect.any(Error)
			);
			errorSpy.mockRestore();
		});
	});
});
