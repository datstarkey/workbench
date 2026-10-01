import type { StartAgentBody } from '$types/workbench';
import { AgentChat } from '@workbench/chat-ui';
import { loopbackAgentApi } from './agent-api';

/**
 * Chat connections by pane. A chat outlives its view: hidden workspaces
 * unmount `SessionChat`, but the connection stays open so a question or
 * approval in a background chat still reaches the sidebar and notifications.
 * Released when the pane goes back to the terminal or is closed.
 */
const chats = new Map<string, AgentChat>();
/** Session ids this window started or attached to; chat adoption skips them. */
const claimed = new Set<string>();

/** The pane's chat, created on first use or when the pane moved to another session. */
export function acquireChat(
	paneId: string,
	body: StartAgentBody
): { chat: AgentChat; created: boolean } {
	const existing = chats.get(paneId);
	// `/clear` re-keys the same chat; any other new id is a different conversation.
	if (existing && existing.sessionId === body.sessionId) return { chat: existing, created: false };
	existing?.dispose();
	claimed.add(body.sessionId);
	const chat = new AgentChat(body, loopbackAgentApi);
	chats.set(paneId, chat);
	return { chat, created: true };
}

export function isChatClaimed(sessionId: string): boolean {
	return claimed.has(sessionId) || [...chats.values()].some((c) => c.sessionId === sessionId);
}

export function releaseChat(paneId: string): void {
	chats.get(paneId)?.dispose();
	chats.delete(paneId);
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
