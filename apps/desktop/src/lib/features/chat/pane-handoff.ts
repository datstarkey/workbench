import type { AgentKind, TerminalPaneState } from '$types/workbench';
import {
	claudeNewSessionWithIdCommand,
	newSessionCommand,
	tryResumeCommand,
	type LaunchOptions
} from '$lib/utils/claude';

/** The chat agent an AI pane runs (Claude unless it's a Codex pane). */
export const paneAgent = (pane: Pick<TerminalPaneState, 'type'>): AgentKind =>
	pane.type === 'codex' ? 'codex' : 'claude';

/**
 * What a pane's terminal runs once its chat hands the session back. `started`:
 * the conversation has something on disk to resume. A Claude chat picked its
 * id up front, so an empty one starts on that id; an empty Codex thread may
 * never have been written, so the terminal starts a fresh `codex` instead.
 */
export function terminalAfterChat(
	pane: TerminalPaneState,
	started: boolean,
	opts: LaunchOptions
): Partial<TerminalPaneState> {
	const id = pane.claudeSessionId;
	if (paneAgent(pane) === 'codex') {
		const resume = id && started ? tryResumeCommand('codex', id, opts) : undefined;
		return resume
			? { startupCommand: resume }
			: { claudeSessionId: '', startupCommand: newSessionCommand('codex', opts) };
	}
	if (!id) return {};
	const startupCommand = started
		? tryResumeCommand('claude', id, opts)
		: claudeNewSessionWithIdCommand(id, opts);
	return startupCommand ? { startupCommand } : {};
}
