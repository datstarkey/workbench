import type { StartAgentBody } from '$types/workbench';
import { AgentChat } from './agent-chat.svelte';

/**
 * Chat connections by pane. A chat outlives its view: hidden workspaces
 * unmount `SessionChat`, but the connection stays open so a question or
 * approval in a background chat still reaches the sidebar and notifications.
 * Released when the pane goes back to the terminal or is closed.
 */
const chats = new Map<string, AgentChat>();

/** The pane's chat, created on first use or when the pane moved to another session. */
export function acquireChat(
	paneId: string,
	body: StartAgentBody
): { chat: AgentChat; created: boolean } {
	const existing = chats.get(paneId);
	// `/clear` re-keys the same chat; any other new id is a different conversation.
	if (existing && existing.sessionId === body.sessionId) return { chat: existing, created: false };
	existing?.dispose();
	const chat = new AgentChat(body);
	chats.set(paneId, chat);
	return { chat, created: true };
}

export function releaseChat(paneId: string): void {
	chats.get(paneId)?.dispose();
	chats.delete(paneId);
}
