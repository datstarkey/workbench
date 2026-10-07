import type {
	AgentKind,
	AgentSummary,
	ProjectWorkspace,
	TerminalPaneState,
	TerminalTabState
} from '$types/workbench';
import { uid } from '$lib/utils/uid';
import { paneAgent } from '$features/chat/pane-handoff';
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
 * Adopted panes are left out of the persisted snapshot; closing one ends it
 * like an own pane. A pane stops being adopted once it runs its own session here.
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
	/** Workspaces opened in the background to host an adopted chat. */
	private createdWorkspaces = new Set<string>();

	isAdopted(paneId: string): boolean {
		return this.adopted.has(paneId);
	}

	/** The pane now runs its own session here; true when it was adopted. */
	takeOver(paneId: string): boolean {
		return this.adopted.delete(paneId);
	}

	/** A workspace opened only to host an adopted chat. */
	markCreated(workspaceId: string): void {
		this.createdWorkspaces.add(workspaceId);
	}

	/** Opened for adoption and now empty: it closes rather than linger. */
	isAbandoned(ws: ProjectWorkspace): boolean {
		return this.createdWorkspaces.has(ws.id) && ws.terminalTabs.length === 0;
	}

	/**
	 * Workspaces without adopted panes, for persisting. One opened for adoption
	 * is left out until it holds something of this window's own: re-adoption
	 * rebuilds it after a reload.
	 */
	persistable(workspaces: ProjectWorkspace[]): ProjectWorkspace[] {
		return withoutPanes(workspaces, this.adopted).filter(
			(w) => !this.createdWorkspaces.has(w.id) || w.terminalTabs.length > 0
		);
	}

	/** Adopted panes whose tab label lags the chat's title on the server. */
	relabels(
		workspaces: ProjectWorkspace[],
		list: AgentSummary[]
	): { paneId: string; label: string; type: AgentKind }[] {
		const titles = new Map(list.flatMap((c) => (c.title ? [[c.sessionId, c.title]] : [])));
		return workspaces.flatMap((w) =>
			w.terminalTabs.flatMap((t) =>
				t.panes.flatMap((p) => {
					const title = p.claudeSessionId && titles.get(p.claudeSessionId);
					return this.adopted.has(p.id) && title && title !== t.label
						? [{ paneId: p.id, label: title, type: paneAgent(p) }]
						: [];
				})
			)
		);
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
		const type = paneAgent({ type: chat.agent });
		return {
			workspaceId: ws.id,
			tab: {
				id: uid(),
				label: chat.title?.trim() || 'Remote chat',
				split: 'horizontal',
				type,
				panes: [
					{
						id: paneId,
						type,
						claudeSessionId: chat.sessionId,
						view: 'chat',
						...(type === 'claude' &&
							chat.claudeAccountId && { claudeAccountId: chat.claudeAccountId })
					}
				]
			}
		};
	}

	/** Panes still holding a session id from before a `/clear`, with the id it moved to. */
	rekeys(
		workspaces: ProjectWorkspace[],
		list: AgentSummary[]
	): { paneId: string; sessionId: string; type: AgentKind }[] {
		const moved = new Map(
			list.flatMap((c) => c.previousIds.map((id): [string, string] => [id, c.sessionId]))
		);
		return panesOf(workspaces).flatMap((p) => {
			const sessionId = p.claudeSessionId && moved.get(p.claudeSessionId);
			return sessionId ? [{ paneId: p.id, sessionId, type: paneAgent(p) }] : [];
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

	/** A pane is closing: never adopt its sessions again. True when it was adopted. */
	release(pane: TerminalPaneState, serverTerminalId: string | undefined): boolean {
		if (pane.claudeSessionId) this.closedChats.add(pane.claudeSessionId);
		const adopted = this.adopted.delete(pane.id);
		if (adopted && serverTerminalId) this.releasedTerminals.add(serverTerminalId);
		return adopted;
	}
}
