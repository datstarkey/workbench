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

		it('clears previous error when requesting again', async () => {
			const action = new ConfirmAction<string>();
			// Simulate an error state
			action.request('first');
			const failing = vi.fn().mockRejectedValue(new Error('fail'));

			await action.confirm(failing);
			expect(action.error).toBe('Error: fail');

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

	describe('busy', () => {
		function deferred() {
			let resolve!: () => void;
			let reject!: (e: unknown) => void;
			const promise = new Promise<void>((res, rej) => {
				resolve = res;
				reject = rej;
			});
			return { promise, resolve, reject };
		}

		it('is set while the action runs and cleared after success', async () => {
			const action = new ConfirmAction<string>();
			const d = deferred();
			action.request('val');

			const running = action.confirm(() => d.promise);
			expect(action.busy).toBe(true);
			expect(action.open).toBe(true);

			d.resolve();
			await running;
			expect(action.busy).toBe(false);
			expect(action.open).toBe(false);
		});

		it('is cleared after failure, leaving the dialog open for retry', async () => {
			const action = new ConfirmAction<string>();
			const d = deferred();
			action.request('val');

			const running = action.confirm(() => d.promise);
			d.reject(new Error('fail'));
			await running;

			expect(action.busy).toBe(false);
			expect(action.open).toBe(true);
			expect(action.error).toBe('Error: fail');
		});

		it('ignores re-entrant confirms while running', async () => {
			const action = new ConfirmAction<string>();
			const d = deferred();
			const handler = vi.fn(() => d.promise);
			action.request('val');

			const running = action.confirm(handler);
			await action.confirm(handler);
			expect(handler).toHaveBeenCalledTimes(1);

			d.resolve();
			await running;
		});

		it('ignores dismissal, cancel and new requests while running', async () => {
			const action = new ConfirmAction<string>();
			const d = deferred();
			action.request('val');

			const running = action.confirm(() => d.promise);
			action.open = false;
			action.cancel();
			action.request('other');
			expect(action.open).toBe(true);
			expect(action.pendingValue).toBe('val');

			d.resolve();
			await running;
			expect(action.open).toBe(false);
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
