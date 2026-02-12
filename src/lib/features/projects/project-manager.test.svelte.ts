import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { clearInvokeMocks } from '../../../test/tauri-mocks';
import { ProjectManagerStore } from './project-manager.svelte';
import type { GitStore } from '$stores/git.svelte';
import type { ProjectStore } from '$stores/projects.svelte';
import type { WorkspaceStore } from '$stores/workspaces.svelte';
import type { ProjectConfig } from '$types/workbench';

vi.mock('$lib/utils/dialog', () => ({
	selectFolder: vi.fn()
}));

function makeProject(overrides: Partial<ProjectConfig> = {}): ProjectConfig {
	return { name: 'Test', path: '/projects/test', ...overrides };
}

function createMocks() {
	const projectStore = {
		projects: [] as ProjectConfig[],
		getByPath: vi.fn(),
		add: vi.fn(),
		update: vi.fn(),
		openProject: vi.fn(),
		removeWithWorkspaces: vi.fn()
	} as unknown as ProjectStore;

	const workspaceStore = {
		updateProjectInfo: vi.fn()
	} as unknown as WorkspaceStore;

	const gitStore = {
		refreshGitState: vi.fn()
	} as unknown as GitStore;

	return { projectStore, workspaceStore, gitStore };
}

describe('ProjectManagerStore', () => {
	let manager: ProjectManagerStore;
	let mocks: ReturnType<typeof createMocks>;

	beforeEach(() => {
		mocks = createMocks();
		manager = new ProjectManagerStore(mocks.projectStore, mocks.workspaceStore, mocks.gitStore);
	});

	afterEach(() => {
		clearInvokeMocks();
	});

	describe('edit', () => {
		it('populates form from existing project and sets dialogMode to edit', () => {
			const project = makeProject({
				name: 'My App',
				path: '/projects/app',
				shell: '/bin/zsh',
				startupCommand: 'npm start',
				tasks: [{ name: 'Build', command: 'npm run build' }]
			});
			vi.mocked(mocks.projectStore.getByPath).mockReturnValue(project);

			manager.edit('/projects/app');

			expect(manager.dialogMode).toBe('edit');
			expect(manager.dialogOpen).toBe(true);
			expect(manager.form.name).toBe('My App');
			expect(manager.form.path).toBe('/projects/app');
			expect(manager.form.shell).toBe('/bin/zsh');
			expect(manager.form.startupCommand).toBe('npm start');
			expect(manager.form.tasks).toEqual([{ name: 'Build', command: 'npm run build' }]);
			expect(manager.formError).toBe('');
		});

		it('does nothing if project not found', () => {
			vi.mocked(mocks.projectStore.getByPath).mockReturnValue(undefined);

			manager.edit('/nonexistent');

			expect(manager.dialogOpen).toBe(false);
		});

		it('defaults shell and startupCommand to empty strings when undefined', () => {
			const project = makeProject({ name: 'Minimal', path: '/minimal' });
			vi.mocked(mocks.projectStore.getByPath).mockReturnValue(project);

			manager.edit('/minimal');

			expect(manager.form.shell).toBe('');
			expect(manager.form.startupCommand).toBe('');
			expect(manager.form.tasks).toEqual([]);
		});
	});

	describe('save - validation', () => {
		it('shows error when name is empty', async () => {
			manager.form = { name: '', path: '/some/path', shell: '', startupCommand: '', tasks: [] };

			await manager.save();

			expect(manager.formError).toBe('Project name is required.');
			expect(mocks.projectStore.add).not.toHaveBeenCalled();
		});

		it('shows error when name is only whitespace', async () => {
			manager.form = {
				name: '   ',
				path: '/some/path',
				shell: '',
				startupCommand: '',
				tasks: []
			};

			await manager.save();

			expect(manager.formError).toBe('Project name is required.');
		});

		it('shows error when path is empty', async () => {
			manager.form = { name: 'Valid', path: '', shell: '', startupCommand: '', tasks: [] };

			await manager.save();

			expect(manager.formError).toBe('Select a project folder.');
		});

		it('detects duplicate path in create mode', async () => {
			mocks.projectStore.projects = [makeProject({ path: '/existing' })];
			manager.form = {
				name: 'New',
				path: '/existing',
				shell: '',
				startupCommand: '',
				tasks: []
			};

			await manager.save();

			expect(manager.formError).toBe('That folder is already added as a project.');
		});

		it('allows same path when editing the same project', async () => {
			const project = makeProject({ name: 'App', path: '/projects/app' });
			mocks.projectStore.projects = [project];
			vi.mocked(mocks.projectStore.getByPath).mockReturnValue(project);

			manager.edit('/projects/app');
			manager.form = { ...manager.form, name: 'App Renamed' };
			await manager.save();

			expect(manager.formError).toBe('');
			expect(mocks.projectStore.update).toHaveBeenCalled();
		});

		it('validates tasks need both name and command', async () => {
			manager.form = {
				name: 'App',
				path: '/app',
				shell: '',
				startupCommand: '',
				tasks: [{ name: 'Build', command: '' }]
			};

			await manager.save();

			expect(manager.formError).toBe('Each task needs both a name and a command.');
		});

		it('validates task names must be unique', async () => {
			manager.form = {
				name: 'App',
				path: '/app',
				shell: '',
				startupCommand: '',
				tasks: [
					{ name: 'Build', command: 'npm run build' },
					{ name: 'build', command: 'npm run build:prod' }
				]
			};

			await manager.save();

			expect(manager.formError).toBe('Task names must be unique.');
		});

		it('ignores fully empty tasks during validation', async () => {
			manager.form = {
				name: 'App',
				path: '/app',
				shell: '',
				startupCommand: '',
				tasks: [
					{ name: 'Build', command: 'npm run build' },
					{ name: '', command: '' }
				]
			};

			await manager.save();

			expect(manager.formError).toBe('');
			expect(mocks.projectStore.add).toHaveBeenCalledWith(
				expect.objectContaining({
					tasks: [{ name: 'Build', command: 'npm run build' }]
				})
			);
		});
	});

	describe('save - create mode', () => {
		it('calls projectStore.add, openProject, and refreshGitState', async () => {
			manager.form = {
				name: 'New App',
				path: '/projects/new-app',
				shell: '/bin/bash',
				startupCommand: 'npm start',
				tasks: []
			};

			await manager.save();

			expect(mocks.projectStore.add).toHaveBeenCalledWith(
				expect.objectContaining({
					name: 'New App',
					path: '/projects/new-app',
					shell: '/bin/bash',
					startupCommand: 'npm start'
				})
			);
			expect(mocks.projectStore.openProject).toHaveBeenCalledWith('/projects/new-app');
			expect(mocks.gitStore.refreshGitState).toHaveBeenCalledWith('/projects/new-app');
		});

		it('closes dialog and resets form after save', async () => {
			manager.form = {
				name: 'App',
				path: '/projects/app',
				shell: '',
				startupCommand: '',
				tasks: []
			};

			await manager.save();

			expect(manager.dialogOpen).toBe(false);
			expect(manager.form.name).toBe('');
			expect(manager.form.path).toBe('');
		});

		it('omits shell and startupCommand when empty', async () => {
			manager.form = {
				name: 'App',
				path: '/projects/app',
				shell: '',
				startupCommand: '  ',
				tasks: []
			};

			await manager.save();

			const savedProject = vi.mocked(mocks.projectStore.add).mock.calls[0][0];
			expect(savedProject.shell).toBeUndefined();
			expect(savedProject.startupCommand).toBeUndefined();
		});

		it('omits tasks when none remain after filtering', async () => {
			manager.form = {
				name: 'App',
				path: '/projects/app',
				shell: '',
				startupCommand: '',
				tasks: [{ name: '', command: '' }]
			};

			await manager.save();

			const savedProject = vi.mocked(mocks.projectStore.add).mock.calls[0][0];
			expect(savedProject.tasks).toBeUndefined();
		});
	});

	describe('save - edit mode', () => {
		it('calls projectStore.update and workspaceStore.updateProjectInfo', async () => {
			const project = makeProject({ name: 'Old', path: '/projects/old' });
			mocks.projectStore.projects = [project];
			vi.mocked(mocks.projectStore.getByPath).mockReturnValue(project);

			manager.edit('/projects/old');
			manager.form = { ...manager.form, name: 'Updated', path: '/projects/updated' };
			await manager.save();

			expect(mocks.projectStore.update).toHaveBeenCalledWith(
				'/projects/old',
				expect.objectContaining({ name: 'Updated', path: '/projects/updated' })
			);
			expect(mocks.workspaceStore.updateProjectInfo).toHaveBeenCalledWith(
				'/projects/old',
				'/projects/updated',
				'Updated'
			);
		});

		it('does not call projectStore.add or openProject', async () => {
			const project = makeProject({ name: 'Edit Me', path: '/projects/edit' });
			mocks.projectStore.projects = [project];
			vi.mocked(mocks.projectStore.getByPath).mockReturnValue(project);

			manager.edit('/projects/edit');
			await manager.save();

			expect(mocks.projectStore.add).not.toHaveBeenCalled();
			expect(mocks.projectStore.openProject).not.toHaveBeenCalled();
		});
	});

	describe('addTask / removeTask / reorderTask', () => {
		it('addTask appends an empty task', () => {
			manager.form = { name: '', path: '', shell: '', startupCommand: '', tasks: [] };

			manager.addTask();

			expect(manager.form.tasks).toEqual([{ name: '', command: '' }]);
		});

		it('addTask appends to existing tasks', () => {
			manager.form = {
				name: '',
				path: '',
				shell: '',
				startupCommand: '',
				tasks: [{ name: 'Existing', command: 'cmd' }]
			};

			manager.addTask();

			expect(manager.form.tasks).toHaveLength(2);
			expect(manager.form.tasks[1]).toEqual({ name: '', command: '' });
		});

		it('removeTask removes task at index', () => {
			manager.form = {
				name: '',
				path: '',
				shell: '',
				startupCommand: '',
				tasks: [
					{ name: 'A', command: 'a' },
					{ name: 'B', command: 'b' },
					{ name: 'C', command: 'c' }
				]
			};

			manager.removeTask(1);

			expect(manager.form.tasks).toEqual([
				{ name: 'A', command: 'a' },
				{ name: 'C', command: 'c' }
			]);
		});

		it('reorderTask moves a task from one index to another', () => {
			manager.form = {
				name: '',
				path: '',
				shell: '',
				startupCommand: '',
				tasks: [
					{ name: 'A', command: 'a' },
					{ name: 'B', command: 'b' },
					{ name: 'C', command: 'c' }
				]
			};

			manager.reorderTask(0, 2);

			expect(manager.form.tasks.map((t) => t.name)).toEqual(['B', 'C', 'A']);
		});

		it('reorderTask does nothing when from equals to', () => {
			manager.form = {
				name: '',
				path: '',
				shell: '',
				startupCommand: '',
				tasks: [{ name: 'A', command: 'a' }]
			};

			manager.reorderTask(0, 0);

			expect(manager.form.tasks).toEqual([{ name: 'A', command: 'a' }]);
		});
	});

	describe('updateTaskName / updateTaskCommand', () => {
		it('updateTaskName updates the name at given index', () => {
			manager.form = {
				name: '',
				path: '',
				shell: '',
				startupCommand: '',
				tasks: [{ name: 'Old', command: 'cmd' }]
			};

			manager.updateTaskName(0, 'New');

			expect(manager.form.tasks[0].name).toBe('New');
		});

		it('updateTaskCommand updates the command at given index', () => {
			manager.form = {
				name: '',
				path: '',
				shell: '',
				startupCommand: '',
				tasks: [{ name: 'Task', command: 'old-cmd' }]
			};

			manager.updateTaskCommand(0, 'new-cmd');

			expect(manager.form.tasks[0].command).toBe('new-cmd');
		});
	});

	describe('remove / confirmRemove', () => {
		it('remove() opens the removal ConfirmAction', () => {
			manager.remove('/projects/to-remove');

			expect(manager.removal.open).toBe(true);
			expect(manager.removal.pendingValue).toBe('/projects/to-remove');
		});

		it('confirmRemove() delegates to projectStore.removeWithWorkspaces', async () => {
			manager.remove('/projects/to-remove');

			await manager.confirmRemove();

			expect(mocks.projectStore.removeWithWorkspaces).toHaveBeenCalledWith('/projects/to-remove');
			expect(manager.removal.open).toBe(false);
		});

		it('confirmRemove() does nothing if no pending removal', async () => {
			await manager.confirmRemove();

			expect(mocks.projectStore.removeWithWorkspaces).not.toHaveBeenCalled();
		});
	});
});
