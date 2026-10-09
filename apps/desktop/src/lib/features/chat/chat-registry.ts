import type { StartAgentBody } from '$types/workbench';
import { AgentChat, ChatDraft } from '@workbench/chat-ui';
import { loopbackAgentApi } from './agent-api';

/**
 * Chat connections by pane. A chat outlives its view: hidden workspaces
 * unmount `SessionChat`, but the connection stays open so a question or
 * approval in a background chat still reaches the sidebar and notifications.
 * Released when the pane goes back to the terminal or leaves the snapshot.
 * Every chat only attaches: the server's workspace service runs the session.
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
	if (existing && existing.sessionId === body.sessionId) return { chat: existing, created: false };
	existing?.dispose();
	let draft = drafts.get(paneId);
	if (!draft) drafts.set(paneId, (draft = new ChatDraft()));
	const chat = new AgentChat({ ...body, attachOnly: true }, loopbackAgentApi, { draft });
	chats.set(paneId, chat);
	return { chat, created: true };
}

export function releaseChat(paneId: string): void {
	chats.get(paneId)?.dispose();
	chats.delete(paneId);
	const draft = drafts.get(paneId);
	if (draft && !draft.text && draft.images.length === 0 && draft.files.length === 0)
		drafts.delete(paneId);
}
