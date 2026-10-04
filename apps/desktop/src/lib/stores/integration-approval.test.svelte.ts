import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { clearInvokeMocks } from '../../test/tauri-mocks';
import { IntegrationApprovalStore } from './integration-approval.svelte';
import type { WorkbenchSettingsStore } from './workbench-settings.svelte';

// Mock terminal utils
vi.mock('$lib/utils/terminal', () => ({
	checkCodexIntegration: vi.fn(),
	applyCodexIntegration: vi.fn()
}));

import { checkCodexIntegration, applyCodexIntegration } from '$lib/utils/terminal';

const mockCheckCodex = vi.mocked(checkCodexIntegration);
const mockApplyCodex = vi.mocked(applyCodexIntegration);

function createMockSettingsStore(
	overrides: Partial<{
		codexConfigApproved: boolean | null;
	}> = {}
) {
	return {
		codexConfigApproved: overrides.codexConfigApproved ?? null,
		getApproval: vi.fn((type: string) => {
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
		mockCheckCodex.mockReset();
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
		it.each(['shell', 'claude'] as const)(
			'returns true immediately for %s without checking',
			async (type) => {
				const result = await store.ensureIntegration(type);

				expect(result).toBe(true);
				expect(store.open).toBe(false);
				expect(mockCheckCodex).not.toHaveBeenCalled();
				expect(mockApplyCodex).not.toHaveBeenCalled();
			}
		);

		it('returns true without dialog when already approved', async () => {
			mockSettingsStore = createMockSettingsStore({ codexConfigApproved: true });
			store = new IntegrationApprovalStore();
			mockApplyCodex.mockResolvedValue(true);

			const result = await store.ensureIntegration('codex');

			expect(result).toBe(true);
			expect(store.open).toBe(false);
			expect(mockApplyCodex).toHaveBeenCalled();
		});

		it('returns true without dialog when previously skipped', async () => {
			mockSettingsStore = createMockSettingsStore({ codexConfigApproved: false });
			store = new IntegrationApprovalStore();

			const result = await store.ensureIntegration('codex');

			expect(result).toBe(true);
			expect(store.open).toBe(false);
			expect(mockApplyCodex).not.toHaveBeenCalled();
		});

		it('auto-approves when no changes needed', async () => {
			mockCheckCodex.mockResolvedValue({ needsChanges: false, description: '' });

			const result = await store.ensureIntegration('codex');

			expect(result).toBe(true);
			expect(store.open).toBe(false);
			expect(mockSettingsStore.setApproval as ReturnType<typeof vi.fn>).toHaveBeenCalledWith(
				'codex',
				true
			);
		});

		it('shows dialog when changes are needed and never asked', async () => {
			mockCheckCodex.mockResolvedValue({
				needsChanges: true,
				description: 'Need to install hooks'
			});

			// Start ensureIntegration but don't await — it waits for user
			const promise = store.ensureIntegration('codex');
			await flushMicrotasks();

			// Dialog should be open
			expect(store.open).toBe(true);
			expect(store.description).toBe('Need to install hooks');

			// Resolve by approving
			mockApplyCodex.mockResolvedValue(true);
			await store.approve();
			const result = await promise;

			expect(result).toBe(true);
		});
	});

	describe('approve', () => {
		it('resolves promise with true, closes dialog, persists approval', async () => {
			mockCheckCodex.mockResolvedValue({
				needsChanges: true,
				description: 'Changes needed'
			});
			mockApplyCodex.mockResolvedValue(true);

			const promise = store.ensureIntegration('codex');
			await flushMicrotasks();
			expect(store.open).toBe(true);

			await store.approve();

			expect(store.open).toBe(false);
			expect(store.error).toBe('');
			expect(mockApplyCodex).toHaveBeenCalled();
			expect(mockSettingsStore.setApproval as ReturnType<typeof vi.fn>).toHaveBeenCalledWith(
				'codex',
				true
			);

			const result = await promise;
			expect(result).toBe(true);
		});

		it('sets error when apply fails', async () => {
			mockCheckCodex.mockResolvedValue({
				needsChanges: true,
				description: 'Changes needed'
			});
			mockApplyCodex.mockRejectedValue(new Error('Permission denied'));

			const promise = store.ensureIntegration('codex');
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
			mockCheckCodex.mockResolvedValue({
				needsChanges: true,
				description: 'Changes needed'
			});

			const promise = store.ensureIntegration('codex');
			await flushMicrotasks();
			expect(store.open).toBe(true);

			store.skip();

			expect(store.open).toBe(false);
			expect(store.error).toBe('');
			expect(mockSettingsStore.setApproval as ReturnType<typeof vi.fn>).toHaveBeenCalledWith(
				'codex',
				false
			);

			const result = await promise;
			expect(result).toBe(true);
		});
	});

	describe('dismiss', () => {
		it('resolves with false, closes dialog without persisting', async () => {
			mockCheckCodex.mockResolvedValue({
				needsChanges: true,
				description: 'Changes needed'
			});

			const promise = store.ensureIntegration('codex');
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
