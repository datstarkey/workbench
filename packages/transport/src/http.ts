import type {
	Capabilities,
	ControlPlaneCommands,
	ControlPlaneEvents,
	ControlPlaneTransport,
	Unsubscribe
} from './transport.ts';

export interface HttpTransportOptions {
	/** Base URL of the workbench-server, e.g. `http://my-box:4317`. */
	baseUrl: string;
	/** Optional bearer token if the server was started with `--token`. */
	token?: string;
}

type Req = { method: string; path: string; query?: Record<string, string>; body?: unknown };

/**
 * Maps each control-plane command to a concrete `workbench-server` request.
 * Commands the server intentionally does not expose (project/workspace
 * persistence is machine-specific and never synced) are omitted and throw.
 */
function toRequest<K extends keyof ControlPlaneCommands>(
	name: K,
	args: ControlPlaneCommands[K]['args']
): Req {
	const a = (args ?? {}) as Record<string, unknown>;
	switch (name) {
		case 'list_projects':
			return { method: 'GET', path: '/projects' };
		case 'list_worktrees':
			return { method: 'GET', path: '/projects/worktrees', query: { path: String(a.path) } };
		case 'create_worktree':
			return { method: 'POST', path: '/projects/worktrees', body: a.request };
		case 'remove_worktree':
			return { method: 'DELETE', path: '/projects/worktrees', body: a };
		case 'list_branches':
			return { method: 'GET', path: '/projects/branches', query: { path: String(a.path) } };
		case 'git_info':
			return { method: 'GET', path: '/projects/git-info', query: { path: String(a.path) } };
		case 'discover_claude_sessions':
			return {
				method: 'GET',
				path: '/sessions/claude',
				query: { projectPath: String(a.projectPath) }
			};
		case 'discover_codex_sessions':
			return {
				method: 'GET',
				path: '/sessions/codex',
				query: { projectPath: String(a.projectPath) }
			};
		case 'load_claude_settings': {
			const query: Record<string, string> = { scope: String(a.scope) };
			if (a.projectPath) query.projectPath = String(a.projectPath);
			return { method: 'GET', path: '/settings/claude', query };
		}
		case 'load_workbench_settings':
			return { method: 'GET', path: '/settings/workbench' };
		case 'remote_spawn':
			return { method: 'POST', path: '/remote/spawn', body: a };
		case 'remote_sessions':
			return { method: 'GET', path: '/remote/sessions' };
		case 'remote_kill':
			return { method: 'DELETE', path: `/remote/sessions/${encodeURIComponent(String(a.id))}` };
		default:
			throw new Error(`HttpTransport: command "${String(name)}" is not supported by the server`);
	}
}

export function createHttpTransport(opts: HttpTransportOptions): ControlPlaneTransport {
	const base = opts.baseUrl.replace(/\/$/, '');

	const headers = (): Record<string, string> => {
		const h: Record<string, string> = { 'content-type': 'application/json' };
		if (opts.token) h.authorization = `Bearer ${opts.token}`;
		return h;
	};

	const capabilities: Capabilities = { terminalIO: false, nativeDialogs: false };

	// Lazily-opened, multiplexed event stream. The server WebSocket endpoint
	// (`/events`) is not implemented yet; this fans out frames of the shape
	// `{ event, payload }` to per-event subscriber sets once it exists.
	let ws: WebSocket | null = null;
	let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
	const listeners = new Map<string, Set<(payload: unknown) => void>>();

	const scheduleReconnect = () => {
		if (reconnectTimer || listeners.size === 0) return;
		reconnectTimer = setTimeout(() => {
			reconnectTimer = null;
			ensureSocket();
		}, 2000);
	};

	const ensureSocket = () => {
		if (ws) return;
		const wsUrl = base.replace(/^http/, 'ws') + '/events';
		try {
			ws = new WebSocket(wsUrl);
			ws.addEventListener('message', (ev) => {
				try {
					const { event, payload } = JSON.parse(ev.data as string);
					listeners.get(event)?.forEach((cb) => cb(payload));
				} catch {
					/* ignore malformed frames */
				}
			});
			// Reconnect on drop/failure so subscriptions survive server restarts
			// and transient network blips instead of going silently dead.
			ws.addEventListener('close', () => {
				ws = null;
				scheduleReconnect();
			});
			ws.addEventListener('error', () => {
				ws?.close();
			});
		} catch {
			ws = null;
			scheduleReconnect();
		}
	};

	return {
		capabilities,

		async invoke(name, args) {
			const req = toRequest(name, args);
			// Drop missing values so an absent param never serializes as the literal
			// string "undefined" (which would hit the server as a bogus path).
			const cleanQuery = req.query
				? Object.fromEntries(
						Object.entries(req.query).filter(([, v]) => v != null && v !== 'undefined')
					)
				: undefined;
			const qs =
				cleanQuery && Object.keys(cleanQuery).length
					? '?' + new URLSearchParams(cleanQuery).toString()
					: '';
			const res = await fetch(`${base}${req.path}${qs}`, {
				method: req.method,
				headers: headers(),
				body: req.body !== undefined ? JSON.stringify(req.body) : undefined
			});
			if (!res.ok) {
				let message = `${res.status} ${res.statusText}`;
				try {
					const err = await res.json();
					if (err?.error) message = err.error;
				} catch {
					/* keep status text */
				}
				throw new Error(`workbench-server: ${message}`);
			}
			if (res.status === 204) return undefined as never;
			const text = await res.text();
			return (text ? JSON.parse(text) : undefined) as never;
		},

		async subscribe<E extends keyof ControlPlaneEvents>(
			event: E,
			cb: (payload: ControlPlaneEvents[E]) => void
		): Promise<Unsubscribe> {
			ensureSocket();
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
