import type { AgentKind, TerminalPaneState } from '$types/workbench';
import { codexCommand, tryCodexResumeCommand, type LaunchOptions } from '$lib/utils/claude';

/** The chat agent an AI pane runs (Claude unless it's a Codex pane). */
export const paneAgent = (pane: Pick<TerminalPaneState, 'type'>): AgentKind =>
	pane.type === 'codex' ? 'codex' : 'claude';

/**
 * What a Codex pane's terminal runs once its chat hands the thread back.
 * `started`: the thread has something on disk to resume; an empty one may
 * never have been written, so the terminal starts a fresh `codex` instead.
 * (A Claude pane's chat and terminal are one process: nothing is handed back.)
 */
export function codexTerminalAfterChat(
	pane: TerminalPaneState,
	started: boolean,
	opts: LaunchOptions
): Partial<TerminalPaneState> {
	const id = pane.claudeSessionId;
	const resume = id && started ? tryCodexResumeCommand(id, opts) : undefined;
	return resume
		? { startupCommand: resume }
		: { claudeSessionId: '', startupCommand: codexCommand(opts) };
}
