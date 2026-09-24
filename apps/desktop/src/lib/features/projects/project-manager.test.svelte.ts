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
				shell: '/bin/zsh'
			});
			vi.mocked(mocks.projectStore.getByPath).mockReturnValue(project);

			manager.edit('/projects/app');

			expect(manager.dialogMode).toBe('edit');
			expect(manager.dialogOpen).toBe(true);
			expect(manager.form.name).toBe('My App');
			expect(manager.form.path).toBe('/projects/app');
			expect(manager.form.shell).toBe('/bin/zsh');
			expect(manager.formError).toBe('');
		});

		it('does nothing if project not found', () => {
			vi.mocked(mocks.projectStore.getByPath).mockReturnValue(undefined);

			manager.edit('/nonexistent');

			expect(manager.dialogOpen).toBe(false);
		});

		it('defaults shell to an empty string when undefined', () => {
			const project = makeProject({ name: 'Minimal', path: '/minimal' });
			vi.mocked(mocks.projectStore.getByPath).mockReturnValue(project);

			manager.edit('/minimal');

			expect(manager.form.shell).toBe('');
		});
	});

	describe('save - validation', () => {
		it('shows error when name is empty', async () => {
			manager.form = {
				name: '',
				path: '/some/path',
				group: '',
				shell: ''
			};

			await manager.save();

			expect(manager.formError).toBe('Project name is required.');
			expect(mocks.projectStore.add).not.toHaveBeenCalled();
		});

		it('shows error when name is only whitespace', async () => {
			manager.form = {
				name: '   ',
				path: '/some/path',
				group: '',
				shell: ''
			};

			await manager.save();

			expect(manager.formError).toBe('Project name is required.');
		});

		it('shows error when path is empty', async () => {
			manager.form = {
				name: 'Valid',
				path: '',
				group: '',
				shell: ''
			};

			await manager.save();

			expect(manager.formError).toBe('Select a project folder.');
		});

		it('detects duplicate path in create mode', async () => {
			mocks.projectStore.projects = [makeProject({ path: '/existing' })];
			manager.form = {
				name: 'New',
				path: '/existing',
				group: '',
				shell: ''
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
	});

	describe('save - create mode', () => {
		it('calls projectStore.add, openProject, and refreshGitState', async () => {
			manager.form = {
				name: 'New App',
				path: '/projects/new-app',
				group: '',
				shell: '/bin/bash'
			};

			await manager.save();

			expect(mocks.projectStore.add).toHaveBeenCalledWith(
				expect.objectContaining({
					name: 'New App',
					path: '/projects/new-app',
					shell: '/bin/bash'
				})
			);
			expect(mocks.projectStore.openProject).toHaveBeenCalledWith('/projects/new-app');
			expect(mocks.gitStore.refreshGitState).toHaveBeenCalledWith('/projects/new-app');
		});

		it('closes dialog and resets form after save', async () => {
			manager.form = {
				name: 'App',
				path: '/projects/app',
				group: '',
				shell: ''
			};

			await manager.save();

			expect(manager.dialogOpen).toBe(false);
			expect(manager.form.name).toBe('');
			expect(manager.form.path).toBe('');
		});

		it('omits shell when empty', async () => {
			manager.form = {
				name: 'App',
				path: '/projects/app',
				group: '',
				shell: ''
			};

			await manager.save();

			const savedProject = vi.mocked(mocks.projectStore.add).mock.calls[0][0];
			expect(savedProject.shell).toBeUndefined();
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

		it('keeps legacy startupCommand and tasks on disk when editing', async () => {
			const project = makeProject({
				name: 'Legacy',
				path: '/projects/legacy',
				startupCommand: 'npm start',
				tasks: [{ name: 'Build', command: 'npm run build' }]
			});
			mocks.projectStore.projects = [project];
			vi.mocked(mocks.projectStore.getByPath).mockReturnValue(project);

			manager.edit('/projects/legacy');
			manager.form = { ...manager.form, name: 'Renamed' };
			await manager.save();

			expect(mocks.projectStore.update).toHaveBeenCalledWith(
				'/projects/legacy',
				expect.objectContaining({
					name: 'Renamed',
					startupCommand: 'npm start',
					tasks: [{ name: 'Build', command: 'npm run build' }]
				})
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
