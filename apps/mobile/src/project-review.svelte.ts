import type { ControlPlaneTransport } from '@workbench/transport';
import type {
	AgentKind,
	DiscoveredClaudeSession,
	GitFileStatus,
	GitStatusResult
} from '@workbench/types';

export interface ReviewFolder {
	projectPath: string;
	worktreePath?: string;
	name: string;
}

/** Read-only folder review. Request generations stop late responses replacing newer picks. */
export class ProjectReview {
	sessions = $state<DiscoveredClaudeSession[]>([]);
	status = $state<GitStatusResult | null>(null);
	diff = $state<string | null>(null);
	loading = $state(false);
	error = $state<string | null>(null);
	private request = 0;
	constructor(
		private readonly transport: ControlPlaneTransport,
		readonly folder: ReviewFolder
	) {}

	private async run(work: () => Promise<() => void>): Promise<void> {
		const request = ++this.request;
		this.loading = true;
		this.error = null;
		try {
			const apply = await work();
			if (request === this.request) apply();
		} catch (e) {
			if (request === this.request) this.error = e instanceof Error ? e.message : String(e);
		} finally {
			if (request === this.request) this.loading = false;
		}
	}

	history(agent: AgentKind, accountId?: string): Promise<void> {
		this.sessions = [];
		return this.run(async () => {
			const list = await this.transport.invoke(
				agent === 'codex' ? 'discover_codex_sessions' : 'discover_claude_sessions',
				{ projectPath: this.folder.worktreePath ?? this.folder.projectPath }
			);
			return () => {
				this.sessions = list
					.filter((s) => agent === 'codex' || (s.accountId ?? '') === (accountId ?? ''))
					.sort((a, b) => b.timestamp.localeCompare(a.timestamp));
			};
		});
	}

	changes(): Promise<void> {
		this.diff = null;
		return this.run(async () => {
			const status = await this.transport.invoke('git_status', this.args());
			return () => {
				this.status = status;
			};
		});
	}

	preview(file: GitFileStatus, staged: boolean): Promise<void> {
		this.diff = null;
		return this.run(async () => {
			const diff = await this.transport.invoke('git_file_diff', {
				...this.args(),
				file: file.path,
				staged
			});
			return () => {
				this.diff = diff;
			};
		});
	}

	private args() {
		return {
			projectPath: this.folder.projectPath,
			path: this.folder.worktreePath ?? this.folder.projectPath
		};
	}
}
