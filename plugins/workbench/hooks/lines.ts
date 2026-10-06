import type { PermissionRequestDecision, SessionRateLimit } from 'claude-code';

// The stream-json lines the chat folds and the text the plugin frames: no
// engine calls here, those stay in the hooks module.

export type Line = Record<string, unknown>;
export type Answer = {
	behavior?: string;
	message?: string;
	updatedInput?: Record<string, unknown>;
	updatedPermissions?: Extract<
		PermissionRequestDecision,
		{ behavior: 'allow' }
	>['updatedPermissions'];
};

/**
 * Sent when chat prompts put into a turn were never read by it: the model
 * finds them in the conversation already. Core's `QUEUED_NUDGE` hides it.
 */
export const QUEUED_NUDGE = 'Please answer the message I sent while you were working.';
/** How the CLI frames a prompt typed while a turn runs; core's `QUEUED_PROMPT_PREFIX`. */
export const QUEUED_PREFIX = 'The user sent a new message while you were working:\n';
const ATTACHED_FILES = '\n\nAttached files (read each with the Read tool):\n';

const EFFORT_LEVELS = ['low', 'medium', 'high', 'xhigh', 'max'];

/** A `/config` model value as the chat's picker shows it. */
export function modelOption(value: string) {
	const wide = value.endsWith('[1m]');
	const base = value.replace('[1m]', '');
	const name = base === 'opusplan' ? 'Opus Plan' : base.charAt(0).toUpperCase() + base.slice(1);
	return {
		value,
		displayName: wide ? `${name} (1M context)` : name,
		...(base === 'haiku' ? {} : { supportedEffortLevels: EFFORT_LEVELS })
	};
}

// The server turns attachments into `@path` mentions, so a prompt is text.
export function promptText(content: unknown): string {
	if (typeof content === 'string') return content;
	const blocks = Array.isArray(content) ? (content as { type?: string; text?: string }[]) : [];
	return blocks
		.filter((b) => b.type === 'text')
		.map((b) => b.text ?? '')
		.join('\n');
}

// What the transcript reads of a tool's structured result: an edit's patch,
// an Artifact's link. The rest (whole files, outputs) stays off the wire.
const RESULT_KEYS = [
	'structuredPatch',
	'url',
	'title',
	'version',
	'created_from_type',
	'updated',
	'opened'
];

export function slimResult(result: unknown): Line | undefined {
	if (!result || typeof result !== 'object') return undefined;
	const kept = Object.entries(result).filter(([k]) => RESULT_KEYS.includes(k));
	return kept.length ? Object.fromEntries(kept) : undefined;
}

export function askLine(
	requestId: string,
	tool: string,
	input: unknown,
	toolUseId?: string,
	reason?: string,
	suggestions?: unknown
): Line {
	return {
		type: 'control_request',
		request_id: requestId,
		request: {
			subtype: 'can_use_tool',
			tool_name: tool,
			input,
			tool_use_id: toolUseId,
			description: reason,
			permission_suggestions: suggestions
		}
	};
}

/**
 * A prompt naming files to Read: a plugin's prompt never has its `@` mentions
 * (chat images and files) expanded, submitted or appended.
 */
export function withAttachments(text: string): string {
	const files = [...text.matchAll(/(?:^|\s)@(?:"([^"]+)"|(\S+))/g)].map((m) => m[1] ?? m[2]);
	return files.length ? `${text}${ATTACHED_FILES}${files.map((f) => `- ${f}`).join('\n')}` : text;
}

/** A chat prompt put into a running turn, framed as the CLI frames a typed one. */
export function midTurn(text: string): string {
	return `${QUEUED_PREFIX}${withAttachments(text)}\n\nIMPORTANT: After completing your current task, you MUST address the user's message above. Do not ignore it.`;
}

/**
 * One `/mod/ask` reply: the answer, `null` to fall back to the terminal (no
 * chat open, or the server is unreachable), or `undefined` to keep waiting.
 */
export function askAnswer(text: string | undefined): Answer | null | undefined {
	if (text === undefined) return null;
	const reply = JSON.parse(text) as {
		answer?: { response?: { subtype?: string; response?: Answer; error?: string } };
		fallback?: boolean;
	};
	if (reply.fallback) return null;
	const response = reply.answer?.response;
	if (!response) return undefined;
	return response.subtype === 'error'
		? { behavior: 'deny', message: response.error }
		: (response.response ?? { behavior: 'deny' });
}

/** The most used rate-limit window, as the SDK's `rate_limit_event` reports one. */
export function rateLimitLine(windows: readonly SessionRateLimit[]): Line | undefined {
	const top = [...windows].sort((a, b) => b.percentUsed - a.percentUsed)[0];
	if (!top) return undefined;
	// The SDK warns past its own thresholds; 90% is where the chat starts to.
	const status =
		top.percentUsed >= 100 ? 'rejected' : top.percentUsed >= 90 ? 'allowed_warning' : 'allowed';
	const resets = top.resetsAt ? Date.parse(top.resetsAt) : NaN;
	return {
		type: 'rate_limit_event',
		rate_limit_info: {
			status,
			rateLimitType: top.kind,
			utilization: top.percentUsed / 100,
			...(Number.isNaN(resets) ? {} : { resetsAt: Math.floor(resets / 1000) })
		}
	};
}
