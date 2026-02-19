import { uid } from '$lib/utils/uid';
import type {
	AgentAction,
	AgentActionTarget,
	SessionType,
	TerminalPerformanceMode,
	WorkbenchSettings,
	WorktreeStartPoint,
	WorktreeStrategy
} from '$types/workbench';
import { invoke } from '@tauri-apps/api/core';

export class WorkbenchSettingsStore {
	worktreeStrategy: WorktreeStrategy = $state('sibling');
	worktreeFetchBeforeCreate = $state(true);
	worktreeStartPoint: WorktreeStartPoint = $state('auto');
	trelloEnabled = $state(false);
	gitSidebarEnabled = $state(false);
	terminalPerformanceMode: TerminalPerformanceMode = $state('auto');
	terminalTelemetryEnabled = $state(false);
	agentActions: AgentAction[] = $state([]);
	claudeHooksApproved: boolean | null = $state(null);
	codexConfigApproved: boolean | null = $state(null);
	loaded = $state(false);
	saving = $state(false);
	dirty = $state(false);

	readonly runnableActions = $derived.by(() =>
		this.agentActions
			.map((a) => ({ ...a, name: a.name.trim(), prompt: a.prompt.trim() }))
			.filter((a) => a.name.length > 0 && a.prompt.length > 0)
	);

	async load() {
		const settings = await invoke<WorkbenchSettings>('load_workbench_settings');
		this.worktreeStrategy = settings.worktreeStrategy;
		this.worktreeFetchBeforeCreate = settings.worktreeFetchBeforeCreate ?? true;
		this.worktreeStartPoint = settings.worktreeStartPoint ?? 'auto';
		this.trelloEnabled = settings.trelloEnabled;
		this.gitSidebarEnabled = settings.gitSidebarEnabled ?? false;
		this.terminalPerformanceMode = settings.terminalPerformanceMode ?? 'auto';
		this.terminalTelemetryEnabled = settings.terminalTelemetryEnabled ?? false;
		this.agentActions = this.normalizeAgentActions(settings.agentActions);
		this.claudeHooksApproved = settings.claudeHooksApproved ?? null;
		this.codexConfigApproved = settings.codexConfigApproved ?? null;
		this.loaded = true;
		this.dirty = false;
	}

	async save() {
		this.saving = true;
		try {
			await invoke('save_workbench_settings', {
				settings: this.toSettings()
			});
			this.dirty = false;
		} finally {
			this.saving = false;
		}
	}

	setWorktreeStrategy(value: WorktreeStrategy) {
		this.worktreeStrategy = value;
		this.dirty = true;
	}

	setWorktreeFetchBeforeCreate(value: boolean) {
		this.worktreeFetchBeforeCreate = value;
		this.dirty = true;
	}

	setWorktreeStartPoint(value: WorktreeStartPoint) {
		this.worktreeStartPoint = value;
		this.dirty = true;
	}

	setTrelloEnabled(value: boolean) {
		this.trelloEnabled = value;
		this.dirty = true;
	}

	setGitSidebarEnabled(value: boolean) {
		this.gitSidebarEnabled = value;
		this.dirty = true;
	}

	setTerminalPerformanceMode(value: TerminalPerformanceMode) {
		this.terminalPerformanceMode = value;
		this.dirty = true;
	}

	setTerminalTelemetryEnabled(value: boolean) {
		this.terminalTelemetryEnabled = value;
		this.dirty = true;
	}

	addAgentAction() {
		this.agentActions = [
			...this.agentActions,
			{
				id: uid(),
				name: '',
				prompt: '',
				target: 'both',
				category: '',
				tags: []
			}
		];
		this.dirty = true;
	}

	updateAgentAction(
		id: string,
		partial: Partial<Pick<AgentAction, 'name' | 'prompt' | 'target' | 'category' | 'tags'>>
	) {
		this.agentActions = this.agentActions.map((action) =>
			action.id === id ? { ...action, ...partial } : action
		);
		this.dirty = true;
	}

	removeAgentAction(id: string) {
		this.agentActions = this.agentActions.filter((action) => action.id !== id);
		this.dirty = true;
	}

	getApproval(type: SessionType): boolean | null {
		if (type === 'claude') return this.claudeHooksApproved;
		if (type === 'codex') return this.codexConfigApproved;
		return true;
	}

	async setApproval(type: SessionType, approved: boolean) {
		if (type === 'claude') this.claudeHooksApproved = approved;
		else if (type === 'codex') this.codexConfigApproved = approved;
		await invoke('save_workbench_settings', { settings: this.toSettings() });
	}

	private toSettings(): WorkbenchSettings {
		return {
			worktreeStrategy: this.worktreeStrategy,
			worktreeFetchBeforeCreate: this.worktreeFetchBeforeCreate,
			worktreeStartPoint: this.worktreeStartPoint,
			trelloEnabled: this.trelloEnabled,
			gitSidebarEnabled: this.gitSidebarEnabled,
			terminalPerformanceMode: this.terminalPerformanceMode,
			terminalTelemetryEnabled: this.terminalTelemetryEnabled,
			agentActions: this.agentActions,
			claudeHooksApproved: this.claudeHooksApproved,
			codexConfigApproved: this.codexConfigApproved
		};
	}

	private normalizeAgentActions(actions: AgentAction[] | undefined): AgentAction[] {
		const safeActions = Array.isArray(actions) ? actions : [];
		return safeActions.map((action) => ({
			id: action.id || uid(),
			name: action.name ?? '',
			prompt: action.prompt ?? '',
			target: this.normalizeTarget(action.target),
			category: action.category ?? '',
			tags: this.normalizeTags(action.tags)
		}));
	}

	private normalizeTarget(target: string | undefined): AgentActionTarget {
		if (target === 'claude' || target === 'codex' || target === 'both') return target;
		return 'both';
	}

	private normalizeTags(tags: string[] | undefined): string[] {
		if (!Array.isArray(tags)) return [];
		return tags.map((tag) => tag.trim()).filter((tag) => tag.length > 0);
	}
}
