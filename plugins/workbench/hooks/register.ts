import type { AnyEventHook, Register } from 'claude-code';

const REFRESH_TOOLS = new Set(['Bash', 'Write', 'Edit', 'NotebookEdit']);

// Workbench sets both variables on every shell and chat process it starts;
// outside Workbench they are unset and the plugin does nothing. Awaiting the
// POST (the bridge answers once it has handled the event) keeps events in order.
const forward: AnyEventHook = async ($, e, next) => {
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
	return next(e);
};

export const register: Register = (on) => {
	on('classic.SessionStart', forward);
	on('classic.UserPromptSubmit', forward);
	on('classic.Stop', forward);
	on('classic.Notification', forward);
	// The bridge only needs the tool and a Bash command; a Write's input and any
	// tool's response can be whole files.
	on('classic.PostToolUse', ($, e, next) => {
		if (!REFRESH_TOOLS.has(e.tool_name)) return next(e);
		const command = (e.tool_input as { command?: unknown } | undefined)?.command;
		const { tool_response: _response, ...rest } = e;
		return forward($, { ...rest, tool_input: { command } }, () => next(e));
	});
};
