import { invoke } from '@tauri-apps/api/core';
import type {
	ClaudeSettings,
	HookScriptInfo,
	PluginInfo,
	SettingsScope,
	ScopeGroup,
	SkillInfo
} from '$types/claude-settings';

const SCOPES: SettingsScope[] = ['user', 'user-local', 'project', 'project-local'];

export class ClaudeSettingsStore {
	settings: Record<SettingsScope, ClaudeSettings> = $state({
		user: {},
		'user-local': {},
		project: {},
		'project-local': {}
	});

	activeScopeGroup: ScopeGroup = $state('user');
	localOnly = $state(false);

	plugins: PluginInfo[] = $state([]);
	skills: SkillInfo[] = $state([]);
	hookScripts: HookScriptInfo[] = $state([]);

	loaded = $state(false);
	dirty = $state(false);
	saving = $state(false);

	private projectPath: string | null = null;

	get activeScope(): SettingsScope {
		if (this.activeScopeGroup === 'user') {
			return this.localOnly ? 'user-local' : 'user';
		}
		return this.localOnly ? 'project-local' : 'project';
	}

	get currentSettings(): ClaudeSettings {
		return this.settings[this.activeScope];
	}

	setScopeGroup(group: ScopeGroup) {
		this.activeScopeGroup = group;
		this.dirty = false;
	}

	setLocalOnly(local: boolean) {
		this.localOnly = local;
		this.dirty = false;
	}

	async load(projectPath: string | null) {
		this.projectPath = projectPath;
		this.loaded = false;

		// Default to project scope when a project is active
		if (projectPath) {
			this.activeScopeGroup = 'project';
		} else {
			this.activeScopeGroup = 'user';
		}

		const results = await Promise.all(
			SCOPES.map((scope) =>
				invoke<ClaudeSettings>('load_claude_settings', {
					scope,
					projectPath: scope.startsWith('project') ? projectPath : null
				}).catch((e) => {
					console.warn(`[ClaudeSettings] Failed to load scope "${scope}":`, e);
					return {} as ClaudeSettings;
				})
			)
		);

		this.settings = {
			user: results[0],
			'user-local': results[1],
			project: results[2],
			'project-local': results[3]
		};

		const [plugins, skills, hookScripts] = await Promise.all([
			invoke<PluginInfo[]>('list_claude_plugins').catch((e) => {
				console.warn('[ClaudeSettings] Failed to list plugins:', e);
				return [] as PluginInfo[];
			}),
			invoke<SkillInfo[]>('list_claude_skills').catch((e) => {
				console.warn('[ClaudeSettings] Failed to list skills:', e);
				return [] as SkillInfo[];
			}),
			invoke<HookScriptInfo[]>('list_claude_hooks_scripts').catch((e) => {
				console.warn('[ClaudeSettings] Failed to list hook scripts:', e);
				return [] as HookScriptInfo[];
			})
		]);

		this.plugins = plugins;
		this.skills = skills;
		this.hookScripts = hookScripts;
		this.loaded = true;
		this.dirty = false;
	}

	async save() {
		this.saving = true;
		try {
			await invoke('save_claude_settings', {
				scope: this.activeScope,
				projectPath: this.activeScope.startsWith('project') ? this.projectPath : null,
				value: this.settings[this.activeScope]
			});
			this.dirty = false;
		} finally {
			this.saving = false;
		}
	}

	update(partial: Partial<ClaudeSettings>) {
		this.settings[this.activeScope] = { ...this.settings[this.activeScope], ...partial };
		this.dirty = true;
	}

	updateNested<K extends keyof ClaudeSettings>(key: K, value: ClaudeSettings[K]) {
		this.settings[this.activeScope] = { ...this.settings[this.activeScope], [key]: value };
		this.dirty = true;
	}

	addToList(key: 'enabledPlugins' | 'disabledPlugins', item: string) {
		const current = (this.currentSettings[key] as string[] | undefined) ?? [];
		if (!current.includes(item)) {
			this.updateNested(key, [...current, item]);
		}
	}

	removeFromList(key: 'enabledPlugins' | 'disabledPlugins', item: string) {
		const current = (this.currentSettings[key] as string[] | undefined) ?? [];
		this.updateNested(
			key,
			current.filter((v) => v !== item)
		);
	}

	addPermission(type: 'allow' | 'deny' | 'ask', pattern: string) {
		const perms = { ...(this.currentSettings.permissions ?? {}) };
		const list = [...(perms[type] ?? [])];
		if (!list.includes(pattern)) {
			list.push(pattern);
		}
		perms[type] = list;
		this.updateNested('permissions', perms);
	}

	removePermission(type: 'allow' | 'deny' | 'ask', pattern: string) {
		const perms = { ...(this.currentSettings.permissions ?? {}) };
		perms[type] = (perms[type] ?? []).filter((v: string) => v !== pattern);
		this.updateNested('permissions', perms);
	}

	updateSandbox(partial: Record<string, unknown>) {
		const sandbox = { ...(this.currentSettings.sandbox ?? {}), ...partial };
		this.updateNested('sandbox', sandbox);
	}

	addToSandboxList(key: string, value: string) {
		const sandbox = this.currentSettings.sandbox ?? {};
		const current = (sandbox[key] as string[] | undefined) ?? [];
		if (!current.includes(value)) {
			this.updateSandbox({ [key]: [...current, value] });
		}
	}

	removeFromSandboxList(key: string, value: string) {
		const sandbox = this.currentSettings.sandbox ?? {};
		const current = (sandbox[key] as string[] | undefined) ?? [];
		this.updateSandbox({ [key]: current.filter((v) => v !== value) });
	}

	updateSandboxNetwork(partial: Record<string, unknown>) {
		const sandbox = { ...(this.currentSettings.sandbox ?? {}) };
		sandbox.network = { ...(sandbox.network ?? {}), ...partial };
		this.updateNested('sandbox', sandbox);
	}

	addToSandboxNetworkList(key: string, value: string) {
		const network = this.currentSettings.sandbox?.network ?? {};
		const current = (network[key] as string[] | undefined) ?? [];
		if (!current.includes(value)) {
			this.updateSandboxNetwork({ [key]: [...current, value] });
		}
	}

	removeFromSandboxNetworkList(key: string, value: string) {
		const network = this.currentSettings.sandbox?.network ?? {};
		const current = (network[key] as string[] | undefined) ?? [];
		this.updateSandboxNetwork({ [key]: current.filter((v) => v !== value) });
	}
}
