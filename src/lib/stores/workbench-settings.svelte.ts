import type { SessionType, WorkbenchSettings, WorktreeStrategy } from '$types/workbench';
import { invoke } from '@tauri-apps/api/core';

export class WorkbenchSettingsStore {
	worktreeStrategy: WorktreeStrategy = $state('sibling');
	claudeHooksApproved: boolean | null = $state(null);
	codexConfigApproved: boolean | null = $state(null);
	loaded = $state(false);
	saving = $state(false);
	dirty = $state(false);

	async load() {
		const settings = await invoke<WorkbenchSettings>('load_workbench_settings');
		this.worktreeStrategy = settings.worktreeStrategy;
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
			claudeHooksApproved: this.claudeHooksApproved,
			codexConfigApproved: this.codexConfigApproved
		};
	}
}
