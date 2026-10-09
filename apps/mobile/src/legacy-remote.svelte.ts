/*
 * OLD-HOST FALLBACK (delete in Phase 5 with home-stream.ts and the
 * `workspaceApi` check in client.svelte.ts). A host without `workspaceApi`
 * has no workspace model, so this builds one from its chat and terminal lists
 * and carries the phone's commands out on the older routes.
 */
import { agentClient } from '@workbench/chat-ui';
import { DEFAULT_TIMEOUT_MS, withTimeout } from '@workbench/transport';
import type {
	AgentSummary,
	CreateServerTerminalBody,
	ServerTerminalMeta as TerminalMeta
} from '@workbench/types';
import { baseName, pathKey } from './home-format.ts';
import { HomeStream } from './home-stream.ts';
import type { PaneRemote } from './remote.svelte.ts';
import type {
	CommandResult,
	OpenEventSource,
	Workspace,
	WorkspaceCommand,
	WorkspacePane,
	WorkspaceServer
} from './workspace-stream.ts';

/** List refresh while the app is in front and the event stream is down. */
const POLL_MS = 4000;

/** A conversation keeps one pane id across `/clear` (its first id) and before its chat attaches. */
const sessionPaneId = (kind: string, ids: string[]) => `${kind}:${ids[0]}`;

/** Chats and terminals as one pane each, grouped by the folder they run in. */
export function legacyWorkspaces(chats: AgentSummary[], terminals: TerminalMeta[]): Workspace[] {
	const workspaces: Record<string, Workspace> = {};
	const add = (projectPath: string, worktreePath: string | undefined, pane: WorkspacePane) => {
		const key = pathKey(worktreePath ?? projectPath);
		const ws = (workspaces[key] ??= {
			id: key,
			projectPath,
			projectName: baseName(projectPath),
			worktreePath,
			tabs: []
		});
		ws.tabs.push({
			id: pane.id,
			label: pane.title ?? baseName(key),
			kind: pane.kind,
			panes: [pane]
		});
	};
	const live = chats.filter((c) => !c.exited);
	const backing = live.map((c) => c.terminalId);
	for (const c of live) {
		add(c.projectPath, c.worktreePath ?? undefined, {
			id: sessionPaneId(c.agent, [...c.previousIds, c.sessionId]),
			kind: c.agent,
			sessionId: c.sessionId,
			previousIds: c.previousIds,
			...(c.claudeAccountId ? { accountId: c.claudeAccountId } : {}),
			...(c.agent === 'codex' ? { codexMode: 'appServer' as const } : {}),
			terminalId: c.terminalId ?? null,
			status: 'running',
			title: c.title,
			busy: c.busy,
			waiting: c.waiting,
			error: null
		});
	}
	for (const t of terminals) {
		if (backing.includes(t.id)) continue;
		const claude = t.claudeSessionId;
		add(t.cwd, undefined, {
			id: claude ? sessionPaneId('claude', [claude]) : `term:${t.id}`,
			kind: claude ? 'claude' : 'shell',
			...(claude ? { sessionId: claude } : {}),
			terminalId: t.id,
			// A Claude terminal whose plugin hasn't attached has no chat yet.
			status: !t.alive ? 'exited' : claude ? 'starting' : 'running',
			title: t.name ?? null,
			busy: !!t.busy,
			waiting: null,
			error: null
		});
	}
	return Object.values(workspaces).sort((a, b) => a.id.localeCompare(b.id));
}

export class LegacyRemote implements PaneRemote {
	chats = $state.raw<AgentSummary[]>([]);
	terminals = $state.raw<TerminalMeta[]>([]);
	/** Counts list updates: what a command's result reports as its `rev`. */
	private rev = 0;
	online = $state(true);
	workspaces = $derived(legacyWorkspaces(this.chats, this.terminals));
	private readonly server: WorkspaceServer;
	/** Bound to this host: a late call after a switch never reaches the next machine. */
	private readonly agents: ReturnType<typeof agentClient>;
	private readonly stream: HomeStream;
	private timer: ReturnType<typeof setInterval> | null = null;
	private disposed = false;
	/** Bumped by every streamed list: a poll response sent before one is stale. */
	private listsSeen = 0;

	constructor(server: WorkspaceServer, openEventSource?: OpenEventSource) {
		this.server = server;
		this.agents = agentClient(() => ({ baseUrl: server.url, token: server.token }));
		this.stream = new HomeStream(
			{
				agents: (list) => this.setLists({ chats: list }),
				terminals: (list) => this.setLists({ terminals: list }),
				status: (live) => {
					if (live) this.online = true;
				}
			},
			openEventSource
		);
	}

	private setLists(lists: { chats?: AgentSummary[]; terminals?: TerminalMeta[] }): void {
		if (this.disposed) return;
		this.listsSeen++;
		if (lists.chats) this.chats = lists.chats;
		if (lists.terminals) this.terminals = lists.terminals;
		this.rev++;
	}

	follow(on: boolean): void {
		this.stream.follow(on ? this.server : null);
		if (this.timer) clearInterval(this.timer);
		this.timer = null;
		if (!on) return;
		let polling = false;
		// One round at a time (each request times out): a stalled host must not pile requests up.
		this.timer = setInterval(() => {
			if (this.stream.live || polling) return;
			polling = true;
			void this.refresh().finally(() => (polling = false));
		}, POLL_MS);
	}

	dispose(): void {
		this.follow(false);
		this.disposed = true;
	}

	async refresh(): Promise<void> {
		if (this.disposed) return;
		const seen = this.listsSeen;
		const [terminals, chats] = await Promise.all([
			this.fetchTerminals(),
			this.agents.list().catch(() => null)
		]);
		if (this.disposed || seen !== this.listsSeen) return;
		this.setLists({ ...(terminals ? { terminals } : {}), ...(chats ? { chats } : {}) });
	}

	private async fetchTerminals(): Promise<TerminalMeta[] | null> {
		try {
			const { ok, data } = await withTimeout(
				'GET /remote/terminals',
				DEFAULT_TIMEOUT_MS,
				async (signal) => {
					const res = await fetch(`${this.server.url}/remote/terminals`, {
						headers: this.auth(),
						signal
					});
					return { ok: res.ok, data: res.ok ? await res.json() : null };
				}
			);
			if (!this.disposed) this.online = ok;
			// Guard the render: a non-array body would throw.
			return ok ? (Array.isArray(data) ? data : []) : null;
		} catch {
			if (!this.disposed) this.online = false;
			return null;
		}
	}

	private auth(): Record<string, string> {
		return { authorization: `Bearer ${this.server.token}` };
	}

	private pane(id: string): { ws: Workspace; pane: WorkspacePane } | null {
		for (const ws of this.workspaces)
			for (const tab of ws.tabs) {
				const pane = tab.panes.find((p) => p.id === id);
				if (pane) return { ws, pane };
			}
		return null;
	}

	async command(cmd: WorkspaceCommand): Promise<CommandResult> {
		switch (cmd.type) {
			case 'newSession': {
				if (!('projectPath' in cmd)) throw new Error('This machine needs a newer Workbench');
				const where = { projectPath: cmd.projectPath, worktreePath: cmd.worktreePath };
				if (cmd.kind === 'codex') {
					const id = await this.agents.start({ agent: 'codex', ...where, sessionId: cmd.resume });
					await this.refresh();
					return { rev: this.rev, paneId: sessionPaneId('codex', [id]) };
				}
				const sessionId = cmd.kind === 'claude' ? (cmd.resume ?? crypto.randomUUID()) : null;
				const meta = await this.createTerminal({
					...where,
					cols: 80,
					rows: 24,
					name: baseName(cmd.worktreePath ?? cmd.projectPath),
					...(sessionId ? { claudeSession: { id: sessionId } } : {}),
					...(sessionId && cmd.accountId ? { claudeAccountId: cmd.accountId } : {})
				});
				return {
					rev: this.rev,
					paneId: sessionId ? sessionPaneId('claude', [sessionId]) : `term:${meta.id}`
				};
			}
			case 'closePane':
			case 'closeTab':
				await this.end('paneId' in cmd ? cmd.paneId : cmd.tabId);
				return { rev: this.rev };
			case 'restart': {
				const found = this.pane(cmd.tabId);
				if (!found?.pane.sessionId || found.pane.kind === 'shell')
					throw new Error('Only a Claude or Codex session can restart');
				await this.agents.start({
					agent: found.pane.kind,
					projectPath: found.ws.projectPath,
					worktreePath: found.ws.worktreePath,
					sessionId: found.pane.sessionId,
					claudeAccountId: found.pane.accountId
				});
				await this.refresh();
				return { rev: this.rev };
			}
			default:
				throw new Error('This machine needs a newer Workbench');
		}
	}

	/** End a chat's session, else close its terminal (which ends a chat it hosts). */
	private async end(paneId: string): Promise<void> {
		const pane = this.pane(paneId)?.pane;
		if (!pane) return;
		if (pane.sessionId && this.chats.some((c) => c.sessionId === pane.sessionId))
			await this.agents.stop(pane.sessionId, { end: true });
		else if (pane.terminalId) {
			const res = await fetch(
				`${this.server.url}/remote/terminals/${encodeURIComponent(pane.terminalId)}`,
				{ method: 'DELETE', headers: this.auth() }
			);
			if (!res.ok) throw new Error(`the server returned ${res.status}`);
		}
		await this.refresh();
	}

	private async createTerminal(body: CreateServerTerminalBody): Promise<TerminalMeta> {
		const res = await fetch(`${this.server.url}/remote/terminals`, {
			method: 'POST',
			headers: { 'content-type': 'application/json', ...this.auth() },
			body: JSON.stringify(body)
		});
		if (!res.ok) {
			const reason = await res
				.json()
				.then((j: { error?: string }) => j.error)
				.catch(() => undefined);
			throw new Error(reason || `the server returned ${res.status}`);
		}
		const meta: TerminalMeta = await res.json();
		// Shown at once, and kept until the server's list has it too.
		await this.refresh();
		if (!this.terminals.some((t) => t.id === meta.id))
			this.setLists({
				terminals: [
					...this.terminals,
					{ ...meta, claudeSessionId: meta.claudeSessionId ?? body.claudeSession?.id }
				]
			});
		return meta;
	}
}
