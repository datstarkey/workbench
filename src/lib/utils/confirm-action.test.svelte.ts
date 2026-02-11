import { describe, it, expect, vi } from 'vitest';
import { ConfirmAction } from './confirm-action.svelte';

describe('ConfirmAction', () => {
	it('has correct initial state', () => {
		const action = new ConfirmAction<string>();
		expect(action.open).toBe(false);
		expect(action.error).toBe('');
		expect(action.pendingValue).toBeNull();
	});

	describe('request', () => {
		it('sets pending value, clears error, and opens', () => {
			const action = new ConfirmAction<string>();
			action.request('test-value');

			expect(action.pendingValue).toBe('test-value');
			expect(action.error).toBe('');
			expect(action.open).toBe(true);
		});

		it('clears previous error when requesting again', () => {
			const action = new ConfirmAction<string>();
			// Simulate an error state
			action.request('first');
			const failing = vi.fn().mockRejectedValue(new Error('fail'));

			action.confirm(failing);

			// Request again should clear error
			action.request('second');
			expect(action.error).toBe('');
			expect(action.pendingValue).toBe('second');
		});
	});

	describe('confirm', () => {
		it('calls action with pending value and closes on success', async () => {
			const action = new ConfirmAction<string>();
			const handler = vi.fn().mockResolvedValue(undefined);

			action.request('my-value');
			await action.confirm(handler);

			expect(handler).toHaveBeenCalledWith('my-value');
			expect(action.pendingValue).toBeNull();
			expect(action.open).toBe(false);
			expect(action.error).toBe('');
		});

		it('does nothing when no pending value', async () => {
			const action = new ConfirmAction<string>();
			const handler = vi.fn().mockResolvedValue(undefined);

			await action.confirm(handler);

			expect(handler).not.toHaveBeenCalled();
			expect(action.open).toBe(false);
		});

		it('catches errors and sets error state', async () => {
			const action = new ConfirmAction<string>();
			const handler = vi.fn().mockRejectedValue(new Error('something went wrong'));

			action.request('val');
			await action.confirm(handler);

			expect(action.error).toBe('Error: something went wrong');
			// Should remain open with pending value on error
			expect(action.open).toBe(true);
			expect(action.pendingValue).toBe('val');
		});

		it('clears previous error before retrying', async () => {
			const action = new ConfirmAction<string>();

			// First confirm fails
			action.request('val');
			await action.confirm(vi.fn().mockRejectedValue(new Error('fail')));
			expect(action.error).toBe('Error: fail');

			// Second confirm succeeds - error should be cleared
			await action.confirm(vi.fn().mockResolvedValue(undefined));
			expect(action.error).toBe('');
		});
	});

	describe('cancel', () => {
		it('clears pending, error, and closes', () => {
			const action = new ConfirmAction<string>();

			action.request('val');
			action.cancel();

			expect(action.pendingValue).toBeNull();
			expect(action.error).toBe('');
			expect(action.open).toBe(false);
		});

		it('clears error state from a failed confirm', async () => {
			const action = new ConfirmAction<string>();
			action.request('val');
			await action.confirm(vi.fn().mockRejectedValue(new Error('fail')));

			action.cancel();

			expect(action.error).toBe('');
			expect(action.open).toBe(false);
			expect(action.pendingValue).toBeNull();
		});
	});

	describe('works with non-string types', () => {
		it('handles object values', async () => {
			const action = new ConfirmAction<{ id: number; name: string }>();
			const handler = vi.fn().mockResolvedValue(undefined);

			action.request({ id: 42, name: 'test' });
			expect(action.pendingValue).toEqual({ id: 42, name: 'test' });

			await action.confirm(handler);
			expect(handler).toHaveBeenCalledWith({ id: 42, name: 'test' });
		});
	});
});
