import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { invokeSpy, mockInvoke, clearInvokeMocks } from '../../test/tauri-mocks';
import { ClaudeSettingsStore } from './claude-settings.svelte';
import type { ClaudeSettings } from '$types/claude-settings';

function defaultSettings(): ClaudeSettings {
	return {};
}

describe('ClaudeSettingsStore', () => {
	let store: ClaudeSettingsStore;

	beforeEach(() => {
		store = new ClaudeSettingsStore();
		mockInvoke('load_claude_settings', () => defaultSettings());
		mockInvoke('list_claude_plugins', () => []);
		mockInvoke('list_claude_skills', () => []);
		mockInvoke('list_claude_hooks_scripts', () => []);
		mockInvoke('save_claude_settings', () => undefined);
	});

	afterEach(() => {
		clearInvokeMocks();
	});

	describe('activeScope', () => {
		it('returns "user" when scopeGroup=user and localOnly=false', () => {
			store.activeScopeGroup = 'user';
			store.localOnly = false;
			expect(store.activeScope).toBe('user');
		});

		it('returns "user-local" when scopeGroup=user and localOnly=true', () => {
			store.activeScopeGroup = 'user';
			store.localOnly = true;
			expect(store.activeScope).toBe('user-local');
		});

		it('returns "project" when scopeGroup=project and localOnly=false', () => {
			store.activeScopeGroup = 'project';
			store.localOnly = false;
			expect(store.activeScope).toBe('project');
		});

		it('returns "project-local" when scopeGroup=project and localOnly=true', () => {
			store.activeScopeGroup = 'project';
			store.localOnly = true;
			expect(store.activeScope).toBe('project-local');
		});
	});

	describe('currentSettings', () => {
		it('returns settings for the active scope', () => {
			store.settings = {
				user: { language: 'en' },
				'user-local': {},
				project: { language: 'fr' },
				'project-local': {}
			};
			store.activeScopeGroup = 'project';
			store.localOnly = false;
			expect(store.currentSettings).toEqual({ language: 'fr' });
		});
	});

	describe('setScopeGroup', () => {
		it('changes activeScopeGroup and resets dirty', () => {
			store.dirty = true;
			store.setScopeGroup('project');
			expect(store.activeScopeGroup).toBe('project');
			expect(store.dirty).toBe(false);
		});
	});

	describe('setLocalOnly', () => {
		it('changes localOnly and resets dirty', () => {
			store.dirty = true;
			store.setLocalOnly(true);
			expect(store.localOnly).toBe(true);
			expect(store.dirty).toBe(false);
		});
	});

	describe('load', () => {
		it('fetches all 4 scopes + plugins/skills/hooks', async () => {
			const userSettings: ClaudeSettings = { language: 'en' };
			const projectSettings: ClaudeSettings = { language: 'fr' };
			const plugins = [{ name: 'p1', description: 'd', version: '1.0', dirName: 'p1' }];
			const skills = [{ name: 's1', dirName: 's1', description: 'skill' }];
			const hooks = [{ name: 'h1', path: '/hooks/h1' }];

			clearInvokeMocks();
			mockInvoke('load_claude_settings', (args: unknown) => {
				const { scope } = args as { scope: string };
				if (scope === 'user') return userSettings;
				if (scope === 'project') return projectSettings;
				return {};
			});
			mockInvoke('list_claude_plugins', () => plugins);
			mockInvoke('list_claude_skills', () => skills);
			mockInvoke('list_claude_hooks_scripts', () => hooks);

			await store.load('/projects/foo');

			expect(store.settings.user).toEqual(userSettings);
			expect(store.settings.project).toEqual(projectSettings);
			expect(store.plugins).toEqual(plugins);
			expect(store.skills).toEqual(skills);
			expect(store.hookScripts).toEqual(hooks);
			expect(store.loaded).toBe(true);
			expect(store.dirty).toBe(false);
		});

		it('defaults to project scopeGroup when projectPath is provided', async () => {
			await store.load('/projects/foo');
			expect(store.activeScopeGroup).toBe('project');
		});

		it('defaults to user scopeGroup when projectPath is null', async () => {
			await store.load(null);
			expect(store.activeScopeGroup).toBe('user');
		});

		it('handles failures gracefully with fallback empty values', async () => {
			const warnSpy = vi.spyOn(console, 'warn').mockImplementation(() => {});
			clearInvokeMocks();
			mockInvoke('load_claude_settings', () => {
				throw new Error('load failed');
			});
			mockInvoke('list_claude_plugins', () => {
				throw new Error('plugins failed');
			});
			mockInvoke('list_claude_skills', () => {
				throw new Error('skills failed');
			});
			mockInvoke('list_claude_hooks_scripts', () => {
				throw new Error('hooks failed');
			});

			await store.load(null);

			expect(store.settings.user).toEqual({});
			expect(store.plugins).toEqual([]);
			expect(store.skills).toEqual([]);
			expect(store.hookScripts).toEqual([]);
			expect(store.loaded).toBe(true);
			warnSpy.mockRestore();
		});

		it('passes projectPath for project scopes only', async () => {
			await store.load('/projects/foo');

			const calls = invokeSpy.mock.calls.filter((c) => c[0] === 'load_claude_settings');
			const scopeArgs = calls.map((c) => c[1] as { scope: string; projectPath: string | null });

			const userCall = scopeArgs.find((a) => a.scope === 'user');
			const userLocalCall = scopeArgs.find((a) => a.scope === 'user-local');
			const projectCall = scopeArgs.find((a) => a.scope === 'project');
			const projectLocalCall = scopeArgs.find((a) => a.scope === 'project-local');

			expect(userCall?.projectPath).toBeNull();
			expect(userLocalCall?.projectPath).toBeNull();
			expect(projectCall?.projectPath).toBe('/projects/foo');
			expect(projectLocalCall?.projectPath).toBe('/projects/foo');
		});
	});

	describe('save', () => {
		it('invokes save with the correct scope and settings', async () => {
			store.activeScopeGroup = 'user';
			store.localOnly = false;
			store.settings.user = { language: 'en' };

			await store.save();

			expect(invokeSpy).toHaveBeenCalledWith('save_claude_settings', {
				scope: 'user',
				projectPath: null,
				value: { language: 'en' }
			});
		});

		it('passes projectPath for project scopes', async () => {
			await store.load('/projects/foo');
			store.activeScopeGroup = 'project';
			store.localOnly = true;
			store.settings['project-local'] = { language: 'de' };

			await store.save();

			expect(invokeSpy).toHaveBeenCalledWith('save_claude_settings', {
				scope: 'project-local',
				projectPath: '/projects/foo',
				value: { language: 'de' }
			});
		});

		it('manages saving and dirty flags', async () => {
			store.dirty = true;
			const savePromise = store.save();
			expect(store.saving).toBe(true);

			await savePromise;
			expect(store.saving).toBe(false);
			expect(store.dirty).toBe(false);
		});

		it('resets saving flag even on error', async () => {
			clearInvokeMocks();
			mockInvoke('save_claude_settings', () => {
				throw new Error('save failed');
			});

			await expect(store.save()).rejects.toThrow('save failed');
			expect(store.saving).toBe(false);
		});
	});

	describe('update', () => {
		it('merges partial settings into active scope', () => {
			store.activeScopeGroup = 'user';
			store.localOnly = false;
			store.settings.user = { language: 'en' };

			store.update({ showTurnDuration: true });

			expect(store.settings.user).toEqual({ language: 'en', showTurnDuration: true });
			expect(store.dirty).toBe(true);
		});
	});

	describe('updateNested', () => {
		it('sets a specific key in the active scope settings', () => {
			store.activeScopeGroup = 'user';
			store.localOnly = false;
			store.settings.user = {};

			store.updateNested('language', 'fr');

			expect(store.settings.user.language).toBe('fr');
			expect(store.dirty).toBe(true);
		});
	});

	describe('addToList / removeFromList', () => {
		it('adds an item to enabledPlugins', () => {
			store.activeScopeGroup = 'user';
			store.localOnly = false;
			store.settings.user = { enabledPlugins: ['a'] };

			store.addToList('enabledPlugins', 'b');

			expect(store.settings.user.enabledPlugins).toEqual(['a', 'b']);
			expect(store.dirty).toBe(true);
		});

		it('does not add duplicate items', () => {
			store.activeScopeGroup = 'user';
			store.localOnly = false;
			store.settings.user = { enabledPlugins: ['a'] };

			store.addToList('enabledPlugins', 'a');

			expect(store.settings.user.enabledPlugins).toEqual(['a']);
		});

		it('creates list if undefined', () => {
			store.activeScopeGroup = 'user';
			store.localOnly = false;
			store.settings.user = {};

			store.addToList('enabledPlugins', 'x');

			expect(store.settings.user.enabledPlugins).toEqual(['x']);
		});

		it('removes an item from the list', () => {
			store.activeScopeGroup = 'user';
			store.localOnly = false;
			store.settings.user = { disabledPlugins: ['a', 'b', 'c'] };

			store.removeFromList('disabledPlugins', 'b');

			expect(store.settings.user.disabledPlugins).toEqual(['a', 'c']);
			expect(store.dirty).toBe(true);
		});
	});

	describe('addPermission / removePermission', () => {
		it('adds a permission pattern', () => {
			store.activeScopeGroup = 'user';
			store.localOnly = false;
			store.settings.user = {};

			store.addPermission('allow', 'Bash(npm test)');

			expect(store.settings.user.permissions?.allow).toEqual(['Bash(npm test)']);
			expect(store.dirty).toBe(true);
		});

		it('does not add duplicate permission', () => {
			store.activeScopeGroup = 'user';
			store.localOnly = false;
			store.settings.user = { permissions: { allow: ['Bash(npm test)'] } };

			store.addPermission('allow', 'Bash(npm test)');

			expect(store.settings.user.permissions?.allow).toEqual(['Bash(npm test)']);
		});

		it('removes a permission pattern', () => {
			store.activeScopeGroup = 'user';
			store.localOnly = false;
			store.settings.user = { permissions: { deny: ['Read', 'Write'] } };

			store.removePermission('deny', 'Read');

			expect(store.settings.user.permissions?.deny).toEqual(['Write']);
			expect(store.dirty).toBe(true);
		});

		it('handles removing from undefined permissions', () => {
			store.activeScopeGroup = 'user';
			store.localOnly = false;
			store.settings.user = {};

			store.removePermission('allow', 'anything');

			expect(store.settings.user.permissions?.allow).toEqual([]);
		});
	});

	describe('updateSandbox', () => {
		it('merges sandbox settings', () => {
			store.activeScopeGroup = 'user';
			store.localOnly = false;
			store.settings.user = { sandbox: { enabled: true } };

			store.updateSandbox({ autoAllowBashIfSandboxed: true });

			expect(store.settings.user.sandbox).toEqual({
				enabled: true,
				autoAllowBashIfSandboxed: true
			});
			expect(store.dirty).toBe(true);
		});

		it('creates sandbox if undefined', () => {
			store.activeScopeGroup = 'user';
			store.localOnly = false;
			store.settings.user = {};

			store.updateSandbox({ enabled: false });

			expect(store.settings.user.sandbox).toEqual({ enabled: false });
		});
	});

	describe('addToSandboxList / removeFromSandboxList', () => {
		it('adds a value to a sandbox list', () => {
			store.activeScopeGroup = 'user';
			store.localOnly = false;
			store.settings.user = { sandbox: { excludedCommands: ['git'] } };

			store.addToSandboxList('excludedCommands', 'npm');

			expect(store.settings.user.sandbox?.excludedCommands).toEqual(['git', 'npm']);
		});

		it('does not add duplicate value', () => {
			store.activeScopeGroup = 'user';
			store.localOnly = false;
			store.settings.user = { sandbox: { excludedCommands: ['git'] } };

			store.addToSandboxList('excludedCommands', 'git');

			expect(store.settings.user.sandbox?.excludedCommands).toEqual(['git']);
		});

		it('creates list when sandbox is undefined', () => {
			store.activeScopeGroup = 'user';
			store.localOnly = false;
			store.settings.user = {};

			store.addToSandboxList('excludedCommands', 'docker');

			expect(store.settings.user.sandbox?.excludedCommands).toEqual(['docker']);
		});

		it('removes a value from sandbox list', () => {
			store.activeScopeGroup = 'user';
			store.localOnly = false;
			store.settings.user = { sandbox: { excludedCommands: ['git', 'npm', 'bun'] } };

			store.removeFromSandboxList('excludedCommands', 'npm');

			expect(store.settings.user.sandbox?.excludedCommands).toEqual(['git', 'bun']);
		});
	});

	describe('updateSandboxNetwork', () => {
		it('merges network config into sandbox', () => {
			store.activeScopeGroup = 'user';
			store.localOnly = false;
			store.settings.user = {
				sandbox: { enabled: true, network: { allowLocalBinding: false } }
			};

			store.updateSandboxNetwork({ allowLocalBinding: true });

			expect(store.settings.user.sandbox?.network).toEqual({ allowLocalBinding: true });
			expect(store.settings.user.sandbox?.enabled).toBe(true);
		});
	});

	describe('addToSandboxNetworkList / removeFromSandboxNetworkList', () => {
		it('adds a domain to allowedDomains', () => {
			store.activeScopeGroup = 'user';
			store.localOnly = false;
			store.settings.user = {
				sandbox: { network: { allowedDomains: ['github.com'] } }
			};

			store.addToSandboxNetworkList('allowedDomains', 'npm.org');

			expect(store.settings.user.sandbox?.network?.allowedDomains).toEqual([
				'github.com',
				'npm.org'
			]);
		});

		it('does not add duplicate domain', () => {
			store.activeScopeGroup = 'user';
			store.localOnly = false;
			store.settings.user = {
				sandbox: { network: { allowedDomains: ['github.com'] } }
			};

			store.addToSandboxNetworkList('allowedDomains', 'github.com');

			expect(store.settings.user.sandbox?.network?.allowedDomains).toEqual(['github.com']);
		});

		it('removes a domain from allowedDomains', () => {
			store.activeScopeGroup = 'user';
			store.localOnly = false;
			store.settings.user = {
				sandbox: { network: { allowedDomains: ['github.com', 'npm.org'] } }
			};

			store.removeFromSandboxNetworkList('allowedDomains', 'github.com');

			expect(store.settings.user.sandbox?.network?.allowedDomains).toEqual(['npm.org']);
		});
	});
});
