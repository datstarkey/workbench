import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { clearInvokeMocks } from '../../test/tauri-mocks';
import { IntegrationApprovalStore } from './integration-approval.svelte';
import type { WorkbenchSettingsStore } from './workbench-settings.svelte';

// Mock terminal utils
vi.mock('$lib/utils/terminal', () => ({
	checkClaudeIntegration: vi.fn(),
	checkCodexIntegration: vi.fn(),
	applyClaudeIntegration: vi.fn(),
	applyCodexIntegration: vi.fn()
}));

import {
	checkClaudeIntegration,
	checkCodexIntegration,
	applyClaudeIntegration,
	applyCodexIntegration
} from '$lib/utils/terminal';

const mockCheckClaude = vi.mocked(checkClaudeIntegration);
const mockCheckCodex = vi.mocked(checkCodexIntegration);
const mockApplyClaude = vi.mocked(applyClaudeIntegration);
const mockApplyCodex = vi.mocked(applyCodexIntegration);

function createMockSettingsStore(
	overrides: Partial<{
		claudeHooksApproved: boolean | null;
		codexConfigApproved: boolean | null;
	}> = {}
) {
	return {
		claudeHooksApproved: overrides.claudeHooksApproved ?? null,
		codexConfigApproved: overrides.codexConfigApproved ?? null,
		getApproval: vi.fn((type: string) => {
			if (type === 'claude') return overrides.claudeHooksApproved ?? null;
			if (type === 'codex') return overrides.codexConfigApproved ?? null;
			return true;
		}),
		setApproval: vi.fn()
	} as unknown as WorkbenchSettingsStore;
}

// Mock context so getWorkbenchSettingsStore() works outside a component
let mockSettingsStore: WorkbenchSettingsStore;

vi.mock('./context', () => ({
	getWorkbenchSettingsStore: () => mockSettingsStore
}));

describe('IntegrationApprovalStore', () => {
	let store: IntegrationApprovalStore;

	beforeEach(() => {
		mockSettingsStore = createMockSettingsStore();
		store = new IntegrationApprovalStore();
		mockCheckClaude.mockReset();
		mockCheckCodex.mockReset();
		mockApplyClaude.mockReset();
		mockApplyCodex.mockReset();
	});

	afterEach(() => {
		clearInvokeMocks();
	});

	// Helper: flush microtasks so the async ensureIntegration reaches showDialog
	async function flushMicrotasks() {
		await new Promise((r) => setTimeout(r, 0));
	}

	describe('ensureIntegration', () => {
		it('returns true immediately for shell type', async () => {
			const result = await store.ensureIntegration('shell');

			expect(result).toBe(true);
			expect(mockCheckClaude).not.toHaveBeenCalled();
			expect(mockCheckCodex).not.toHaveBeenCalled();
		});

		it('returns true without dialog when already approved', async () => {
			mockSettingsStore = createMockSettingsStore({ claudeHooksApproved: true });
			store = new IntegrationApprovalStore();
			mockApplyClaude.mockResolvedValue(true);

			const result = await store.ensureIntegration('claude');

			expect(result).toBe(true);
			expect(store.open).toBe(false);
			expect(mockApplyClaude).toHaveBeenCalled();
		});

		it('returns true without dialog when previously skipped', async () => {
			mockSettingsStore = createMockSettingsStore({ claudeHooksApproved: false });
			store = new IntegrationApprovalStore();

			const result = await store.ensureIntegration('claude');

			expect(result).toBe(true);
			expect(store.open).toBe(false);
			expect(mockApplyClaude).not.toHaveBeenCalled();
		});

		it('auto-approves when no changes needed', async () => {
			mockCheckClaude.mockResolvedValue({ needsChanges: false, description: '' });

			const result = await store.ensureIntegration('claude');

			expect(result).toBe(true);
			expect(store.open).toBe(false);
			expect(mockSettingsStore.setApproval as ReturnType<typeof vi.fn>).toHaveBeenCalledWith(
				'claude',
				true
			);
		});

		it('shows dialog when changes are needed and never asked', async () => {
			mockCheckClaude.mockResolvedValue({
				needsChanges: true,
				description: 'Need to install hooks'
			});

			// Start ensureIntegration but don't await — it waits for user
			const promise = store.ensureIntegration('claude');
			await flushMicrotasks();

			// Dialog should be open
			expect(store.open).toBe(true);
			expect(store.description).toBe('Need to install hooks');
			expect(store.sessionType).toBe('claude');

			// Resolve by approving
			mockApplyClaude.mockResolvedValue(true);
			await store.approve();
			const result = await promise;

			expect(result).toBe(true);
		});

		it('uses codex check for codex type', async () => {
			mockCheckCodex.mockResolvedValue({ needsChanges: false, description: '' });

			await store.ensureIntegration('codex');

			expect(mockCheckCodex).toHaveBeenCalled();
			expect(mockCheckClaude).not.toHaveBeenCalled();
		});
	});

	describe('approve', () => {
		it('resolves promise with true, closes dialog, persists approval', async () => {
			mockCheckClaude.mockResolvedValue({
				needsChanges: true,
				description: 'Changes needed'
			});
			mockApplyClaude.mockResolvedValue(true);

			const promise = store.ensureIntegration('claude');
			await flushMicrotasks();
			expect(store.open).toBe(true);

			await store.approve();

			expect(store.open).toBe(false);
			expect(store.error).toBe('');
			expect(mockApplyClaude).toHaveBeenCalled();
			expect(mockSettingsStore.setApproval as ReturnType<typeof vi.fn>).toHaveBeenCalledWith(
				'claude',
				true
			);

			const result = await promise;
			expect(result).toBe(true);
		});

		it('sets error when apply fails', async () => {
			mockCheckClaude.mockResolvedValue({
				needsChanges: true,
				description: 'Changes needed'
			});
			mockApplyClaude.mockRejectedValue(new Error('Permission denied'));

			const promise = store.ensureIntegration('claude');
			await flushMicrotasks();
			expect(store.open).toBe(true);

			await store.approve();

			// Dialog stays open on error
			expect(store.open).toBe(true);
			expect(store.error).toBe('Permission denied');

			// Clean up: dismiss to resolve the promise
			store.dismiss();
			await promise;
		});
	});

	describe('skip', () => {
		it('resolves promise with true and persists skip', async () => {
			mockCheckClaude.mockResolvedValue({
				needsChanges: true,
				description: 'Changes needed'
			});

			const promise = store.ensureIntegration('claude');
			await flushMicrotasks();
			expect(store.open).toBe(true);

			store.skip();

			expect(store.open).toBe(false);
			expect(store.error).toBe('');
			expect(mockSettingsStore.setApproval as ReturnType<typeof vi.fn>).toHaveBeenCalledWith(
				'claude',
				false
			);

			const result = await promise;
			expect(result).toBe(true);
		});
	});

	describe('dismiss', () => {
		it('resolves with false, closes dialog without persisting', async () => {
			mockCheckClaude.mockResolvedValue({
				needsChanges: true,
				description: 'Changes needed'
			});

			const promise = store.ensureIntegration('claude');
			await flushMicrotasks();
			expect(store.open).toBe(true);

			store.dismiss();

			expect(store.open).toBe(false);
			expect(store.error).toBe('');
			expect(mockSettingsStore.setApproval as ReturnType<typeof vi.fn>).not.toHaveBeenCalled();

			const result = await promise;
			expect(result).toBe(false);
		});
	});
});
