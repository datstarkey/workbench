import type { WorkbenchSettings, WorktreeStrategy } from '$types/workbench';
import { invoke } from '@tauri-apps/api/core';

export class WorkbenchSettingsStore {
	worktreeStrategy: WorktreeStrategy = $state('sibling');
	loaded = $state(false);
	saving = $state(false);
	dirty = $state(false);

	async load() {
		const settings = await invoke<WorkbenchSettings>('load_workbench_settings');
		this.worktreeStrategy = settings.worktreeStrategy;
		this.loaded = true;
		this.dirty = false;
	}

	async save() {
		this.saving = true;
		try {
			await invoke('save_workbench_settings', {
				settings: { worktreeStrategy: this.worktreeStrategy } satisfies WorkbenchSettings
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
}
