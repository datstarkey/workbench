import { vi } from 'vitest';

// --- @tauri-apps/api/core ---

const invokeHandlers = new Map<string, (...args: unknown[]) => unknown>();

/** Spy for `invoke()`. By default resolves undefined; override per-command with `mockInvoke`. */
export const invokeSpy = vi.fn(async (cmd: string, args?: unknown) => {
	const handler = invokeHandlers.get(cmd);
	if (handler) return handler(args);
	return undefined;
});

/** Register a per-command mock for `invoke()`. */
export function mockInvoke(command: string, handler: (...args: unknown[]) => unknown) {
	invokeHandlers.set(command, handler);
}

/** Clear all per-command mocks (call in afterEach). */
export function clearInvokeMocks() {
	invokeHandlers.clear();
	invokeSpy.mockClear();
}

vi.mock('@tauri-apps/api/core', () => ({
	invoke: invokeSpy
}));

// --- @tauri-apps/api/event ---

type ListenCallback = (event: { payload: unknown }) => void;
const listeners = new Map<string, ListenCallback[]>();

export const listenSpy = vi.fn(async (event: string, cb: ListenCallback) => {
	const cbs = listeners.get(event) ?? [];
	cbs.push(cb);
	listeners.set(event, cbs);
	return () => {
		const list = listeners.get(event);
		if (list)
			listeners.set(
				event,
				list.filter((fn) => fn !== cb)
			);
	};
});

/** Emit a Tauri event to all registered listeners (for testing). */
export function emitMockEvent(event: string, payload: unknown) {
	const cbs = listeners.get(event) ?? [];
	for (const cb of cbs) cb({ payload });
}

/** Clear all listeners (call in afterEach). */
export function clearListeners() {
	listeners.clear();
	listenSpy.mockClear();
}

vi.mock('@tauri-apps/api/event', () => ({
	listen: listenSpy
}));
