import { uid } from '$lib/utils/uid';
import type {
	AgentAction,
	AgentActionTarget,
	WorkbenchSettings,
	WorktreeStrategy
} from '$types/workbench';
import { invoke } from '@tauri-apps/api/core';

export class WorkbenchSettingsStore {
	worktreeStrategy: WorktreeStrategy = $state('sibling');
	agentActions: AgentAction[] = $state([]);
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
		this.agentActions = this.normalizeAgentActions(settings.agentActions);
		this.loaded = true;
		this.dirty = false;
	}

	async save() {
		this.saving = true;
		try {
			await invoke('save_workbench_settings', {
				settings: {
					worktreeStrategy: this.worktreeStrategy,
					agentActions: this.agentActions
				} satisfies WorkbenchSettings
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
