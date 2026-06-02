import type {
	ControlPlaneCommands,
	ControlPlaneEvents,
	ControlPlaneTransport,
	Unsubscribe
} from './transport.ts';

type Handler = (args: unknown) => unknown | Promise<unknown>;

export interface MockTransport extends ControlPlaneTransport {
	/** Register/override the handler for a command. */
	mockInvoke(name: keyof ControlPlaneCommands, handler: Handler): void;
	/** Push an event payload to all current subscribers. */
	emitMockEvent(event: keyof ControlPlaneEvents, payload: unknown): void;
}

/**
 * In-memory transport for tests — mirrors the old `tauri-mocks` helper. Replaces
 * `vi.mock('@tauri-apps/api/core')` with a transport you inject into stores.
 */
export function createMockTransport(): MockTransport {
	const handlers = new Map<string, Handler>();
	const listeners = new Map<string, Set<(payload: unknown) => void>>();

	return {
		capabilities: { terminalIO: true, nativeDialogs: true },

		mockInvoke(name, handler) {
			handlers.set(name as string, handler);
		},

		emitMockEvent(event, payload) {
			listeners.get(event as string)?.forEach((cb) => cb(payload));
		},

		async invoke(name, args) {
			const handler = handlers.get(name as string);
			if (!handler) throw new Error(`MockTransport: no handler for "${String(name)}"`);
			return (await handler(args)) as never;
		},

		async subscribe(event, cb): Promise<Unsubscribe> {
			const key = event as string;
			const set = listeners.get(key) ?? new Set();
			set.add(cb as (p: unknown) => void);
			listeners.set(key, set);
			return () => {
				listeners.get(key)?.delete(cb as (p: unknown) => void);
			};
		}
	};
}
