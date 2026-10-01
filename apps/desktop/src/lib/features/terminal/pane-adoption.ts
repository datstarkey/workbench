import type {
	AgentSummary,
	ProjectWorkspace,
	TerminalPaneState,
	TerminalTabState
} from '$types/workbench';
import { uid } from '$lib/utils/uid';
import {
	adoptableChats,
	adoptionWorkspace,
	withoutPanes,
	type AdoptableTerminal
} from './server-terminals';

export interface AdoptedTab {
	workspaceId: string;
	tab: TerminalTabState;
}

const panesOf = (workspaces: ProjectWorkspace[]): TerminalPaneState[] =>
	workspaces.flatMap((w) => w.terminalTabs.flatMap((t) => t.panes));

/**
 * Bookkeeping for panes adopted from another device's terminals and chats.
 * Adopted panes are left out of the persisted snapshot and closing one only
 * detaches; a pane stops being adopted once it runs its own session here.
 */
export class PaneAdoption {
	private adopted = new Set<string>();
	/** Server terminals whose adopted tab was closed: never re-adopt them. */
	private releasedTerminals = new Set<string>();
	/**
	 * Chat sessions whose pane closed here, kept while the server still lists
	 * them (a stopping chat, or one the other device keeps running), so a
	 * later continuation on that device can be adopted again.
	 */
	private closedChats = new Set<string>();

	isAdopted(paneId: string): boolean {
		return this.adopted.has(paneId);
	}

	/** The pane now runs its own session here; true when it was adopted. */
	takeOver(paneId: string): boolean {
		return this.adopted.delete(paneId);
	}

	persistable(workspaces: ProjectWorkspace[]): ProjectWorkspace[] {
		return withoutPanes(workspaces, this.adopted);
	}

	/** Server terminal ids the poller must skip: mapped to panes or released. */
	knownTerminalIds(serverTerminalIds: Record<string, string>): string[] {
		return [...Object.values(serverTerminalIds), ...this.releasedTerminals];
	}

	/** A background tab for a terminal opened on another device, if a workspace runs in its cwd. */
	terminalTab(workspaces: ProjectWorkspace[], terminal: AdoptableTerminal): AdoptedTab | null {
		const ws = adoptionWorkspace(workspaces, terminal.cwd);
		if (!ws) return null;
		const paneId = uid();
		this.adopted.add(paneId);
		return {
			workspaceId: ws.id,
			tab: {
				id: uid(),
				label: terminal.name?.trim() || 'Remote terminal',
				split: 'horizontal',
				panes: [{ id: paneId }]
			}
		};
	}

	/** A background chat tab on a session started on another device. */
	chatTab(workspaces: ProjectWorkspace[], chat: AgentSummary): AdoptedTab | null {
		const ws = adoptionWorkspace(workspaces, chat.worktreePath ?? chat.projectPath);
		if (!ws) return null;
		const paneId = uid();
		this.adopted.add(paneId);
		return {
			workspaceId: ws.id,
			tab: {
				id: uid(),
				label: chat.title?.trim() || 'Remote chat',
				split: 'horizontal',
				type: 'claude',
				panes: [
					{
						id: paneId,
						type: 'claude',
						claudeSessionId: chat.sessionId,
						view: 'chat',
						...(chat.claudeAccountId && { claudeAccountId: chat.claudeAccountId })
					}
				]
			}
		};
	}

	/** Panes still holding a session id from before a `/clear`, with the id it moved to. */
	rekeys(
		workspaces: ProjectWorkspace[],
		list: AgentSummary[]
	): { paneId: string; sessionId: string }[] {
		const moved = new Map(
			list.flatMap((c) => c.previousIds.map((id): [string, string] => [id, c.sessionId]))
		);
		return panesOf(workspaces).flatMap((p) => {
			const sessionId = p.claudeSessionId && moved.get(p.claudeSessionId);
			return sessionId ? [{ paneId: p.id, sessionId }] : [];
		});
	}

	/** Listed chats no pane shows, none closed here and none a live chat holds. */
	adoptableChats(
		workspaces: ProjectWorkspace[],
		list: AgentSummary[],
		isClaimed: (sessionId: string) => boolean
	): AgentSummary[] {
		const listed = new Set(list.flatMap((c) => [c.sessionId, ...c.previousIds]));
		for (const id of this.closedChats) if (!listed.has(id)) this.closedChats.delete(id);
		const panes = panesOf(workspaces);
		const known = new Set([
			...panes.map((p) => p.id),
			...panes.flatMap((p) => (p.claudeSessionId ? [p.claudeSessionId] : [])),
			...this.closedChats
		]);
		return adoptableChats(list, known, isClaimed);
	}

	/**
	 * A pane is closing. True when its server sessions are this window's to end;
	 * an adopted pane's belong to the other device, so it only lets go of them.
	 */
	release(pane: TerminalPaneState, serverTerminalId: string | undefined): boolean {
		if (pane.claudeSessionId) this.closedChats.add(pane.claudeSessionId);
		const adopted = this.adopted.delete(pane.id);
		if (adopted && serverTerminalId) this.releasedTerminals.add(serverTerminalId);
		return !adopted;
	}
}
