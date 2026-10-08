import type { EngineInterface, Register } from 'claude-code';
import {
	noteBackgroundTasks,
	notePermissionMode,
	noteTitle,
	register as registerChat
} from './chat';

const REFRESH_TOOLS = new Set(['Bash', 'Write', 'Edit', 'NotebookEdit']);

// How long a hook waits on the desktop's hook bridge, and how long it then
// doesn't wait at all once the bridge didn't answer in time.
const BRIDGE_MS = 3_000;
const BRIDGE_PAUSE_MS = 30_000;
let bridgeSlowUntil = 0;

// Workbench sets both variables on every shell and chat process it starts;
// outside Workbench they are unset and the plugin does nothing. The socket is
// `host:port#secret`, the secret proving the post comes from a process
// Workbench started. Awaiting the POST (the bridge answers once it has
// handled the event) keeps events in order; the classic hooks it runs in gate
// the prompt, and a `$` call doesn't spend their budget, so a hung fetch is
// raced against a clock wait (which does, at most `BRIDGE_MS`); after one
// that ran out, events are still sent but not waited on for a while, rather
// than costing every prompt the wait.
async function forward<R>(
	$: EngineInterface,
	e: { permission_mode?: string; [field: string]: unknown },
	next: () => Promise<R>
): Promise<R> {
	notePermissionMode(e.permission_mode);
	const socket = await $.env.get('WORKBENCH_HOOK_SOCKET');
	const paneId = await $.env.get('WORKBENCH_PANE_ID');
	if (socket && paneId) {
		const [address, secret = ''] = socket.split('#');
		const post = $.http
			.fetch(`http://${address}/hook`, {
				method: 'POST',
				headers: { 'content-type': 'application/json', 'x-workbench-hook-secret': secret },
				body: JSON.stringify({ pane_id: paneId, hook: e })
			})
			.then(
				() => true,
				() => true
			);
		if (Date.now() >= bridgeSlowUntil) {
			const waited = new AbortController();
			const answered = await Promise.race([
				post,
				$.clock.sleep(BRIDGE_MS, { signal: waited.signal }).then(
					() => false,
					() => true
				)
			]);
			waited.abort();
			if (!answered) bridgeSlowUntil = Date.now() + BRIDGE_PAUSE_MS;
		}
	}
	return next();
}

export const register: Register = (on, options) => {
	registerChat(on, options);
	// Both carry the session's title: a generated one reaches chat at the next prompt.
	on('classic.SessionStart', ($, e, next) => {
		noteTitle(e.session_title);
		return forward($, e, () => next(e));
	});
	on('classic.UserPromptSubmit', ($, e, next) => {
		noteTitle(e.session_title);
		return forward($, e, () => next(e));
	});
	on('classic.Stop', ($, e, next) => {
		noteBackgroundTasks(e.background_tasks);
		return forward($, e, () => next(e));
	});
	// Approvals and MCP forms reach both devices through the mod link (the
	// desktop isn't sent Notification).
	on('classic.Notification', ($, e, next) => {
		notePermissionMode(e.permission_mode);
		return next(e);
	});
	// The bridge only needs the tool and a Bash command; a Write's input and any
	// tool's response can be whole files.
	on('classic.PostToolUse', ($, e, next) => {
		if (!REFRESH_TOOLS.has(e.tool_name)) return next(e);
		const command = (e.tool_input as { command?: unknown } | undefined)?.command;
		const { tool_response: _response, ...rest } = e;
		return forward($, { ...rest, tool_input: { command } }, () => next(e));
	});
};
