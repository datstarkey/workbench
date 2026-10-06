import type { StartAgentBody } from '$types/workbench';
import { AgentChat, ChatDraft } from '@workbench/chat-ui';
import { loopbackAgentApi } from './agent-api';

/**
 * Chat connections by pane. A chat outlives its view: hidden workspaces
 * unmount `SessionChat`, but the connection stays open so a question or
 * approval in a background chat still reaches the sidebar and notifications.
 * Released when the pane goes back to the terminal or is closed.
 */
const chats = new Map<string, AgentChat>();
/**
 * Composer drafts by pane. They outlive the chat (Terminal | Chat toggles
 * release it); an emptied one is dropped on release, so a closed pane leaves
 * at most its unsent draft in memory until the app restarts.
 */
const drafts = new Map<string, ChatDraft>();

/** The pane's chat, created on first use or when the pane moved to another session. */
export function acquireChat(
	paneId: string,
	body: StartAgentBody
): { chat: AgentChat; created: boolean } {
	const existing = chats.get(paneId);
	// `/clear` re-keys the same chat; any other new id is a different conversation.
	// No id is a new Codex thread: the chat already starting it keeps it.
	const same =
		body.sessionId === undefined
			? existing?.agent === (body.agent ?? 'claude')
			: existing?.sessionId === body.sessionId;
	if (existing && same) return { chat: existing, created: false };
	existing?.dispose();
	let draft = drafts.get(paneId);
	if (!draft) drafts.set(paneId, (draft = new ChatDraft()));
	const chat = new AgentChat(body, loopbackAgentApi, { draft });
	chats.set(paneId, chat);
	return { chat, created: true };
}

/**
 * A chat in this window holds the session, e.g. it re-keyed (`/clear`) before
 * its pane did. Chat adoption skips it.
 */
export function isChatClaimed(sessionId: string): boolean {
	return [...chats.values()].some((c) => c.sessionId === sessionId);
}

/** Re-attach the pane's chat (a Restart of a chat another device owns). */
export function reopenChat(paneId: string): void {
	void chats.get(paneId)?.open();
}

export function releaseChat(paneId: string): void {
	chats.get(paneId)?.dispose();
	chats.delete(paneId);
	const draft = drafts.get(paneId);
	if (draft && !draft.text && draft.images.length === 0 && draft.files.length === 0)
		drafts.delete(paneId);
}

/**
 * The pane's chat has a conversation on disk to resume. Unknown (no chat in
 * this window, e.g. after a restart) counts as yes: resuming is the safe guess
 * for a pane that was in chat before.
 */
export function chatHasHistory(paneId: string): boolean {
	const chat = chats.get(paneId);
	return !chat || chat.hasHistory;
}
