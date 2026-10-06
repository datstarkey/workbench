import type { EngineInterface, Register } from 'claude-code';
import {
	noteBackgroundTasks,
	notePermissionMode,
	noteTerminalElicitation,
	register as registerChat
} from './chat';

const REFRESH_TOOLS = new Set(['Bash', 'Write', 'Edit', 'NotebookEdit']);

// Workbench sets both variables on every shell and chat process it starts;
// outside Workbench they are unset and the plugin does nothing. Awaiting the
// POST (the bridge answers once it has handled the event) keeps events in order.
async function forward<R>(
	$: EngineInterface,
	e: { permission_mode?: string; [field: string]: unknown },
	next: () => Promise<R>
): Promise<R> {
	notePermissionMode(e.permission_mode);
	const socket = await $.env.get('WORKBENCH_HOOK_SOCKET');
	const paneId = await $.env.get('WORKBENCH_PANE_ID');
	if (socket && paneId) {
		await $.http
			.fetch(`http://${socket}/hook`, {
				method: 'POST',
				headers: { 'content-type': 'application/json' },
				body: JSON.stringify({ pane_id: paneId, hook: e })
			})
			.catch(() => {});
	}
	return next();
}

export const register: Register = (on, options) => {
	registerChat(on, options);
	on('classic.SessionStart', ($, e, next) => forward($, e, () => next(e)));
	on('classic.UserPromptSubmit', ($, e, next) => forward($, e, () => next(e)));
	on('classic.Stop', ($, e, next) => {
		noteBackgroundTasks(e.background_tasks);
		return forward($, e, () => next(e));
	});
	// Approvals reach both devices through the mod link; only an MCP form the
	// terminal shows needs naming here (the desktop isn't sent Notification).
	on('classic.Notification', ($, e, next) => {
		const n = e as { notification_type?: string; message?: string; permission_mode?: string };
		notePermissionMode(n.permission_mode);
		if (n.notification_type === 'elicitation_dialog') noteTerminalElicitation(n.message);
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
