import { selectFolder } from '$lib/utils/dialog';
import { ConfirmAction } from '$lib/utils/confirm-action.svelte';
import { baseName } from '$lib/utils/path';
import type { GitStore } from '$stores/git.svelte';
import type { ProjectStore } from '$stores/projects.svelte';
import type { WorkspaceStore } from '$stores/workspaces.svelte';
import type { ProjectConfig, ProjectFormState } from '$types/workbench';

export class ProjectManagerStore {
	dialogOpen = $state(false);
	dialogMode: 'create' | 'edit' = $state('create');
	groupDialogOpen = $state(false);
	groupDialogValue = $state('');
	private groupDialogProjectPath: string | null = null;
	form: ProjectFormState = $state({
		name: '',
		path: '',
		group: '',
		shell: ''
	});
	formError = $state('');
	readonly removal = new ConfirmAction<string>();

	private editingProjectPath: string | null = null;
	private projectStore: ProjectStore;
	private workspaceStore: WorkspaceStore;
	private gitStore: GitStore;

	constructor(projectStore: ProjectStore, workspaceStore: WorkspaceStore, gitStore: GitStore) {
		this.projectStore = projectStore;
		this.workspaceStore = workspaceStore;
		this.gitStore = gitStore;
	}

	private resetForm() {
		this.form = { name: '', path: '', group: '', shell: '' };
		this.formError = '';
		this.editingProjectPath = null;
	}

	async add() {
		this.dialogMode = 'create';
		this.resetForm();
		this.dialogOpen = true;
		this.formError = 'Choose a project folder to continue.';

		const selectedPath = await selectFolder();
		if (!selectedPath) return;
		this.form = { ...this.form, path: selectedPath, name: baseName(selectedPath) };
		this.formError = '';
	}

	edit(projectPath: string) {
		const project = this.projectStore.getByPath(projectPath);
		if (!project) return;
		this.dialogMode = 'edit';
		this.editingProjectPath = project.path;
		this.form = {
			name: project.name,
			path: project.path,
			group: project.group || '',
			shell: project.shell || ''
		};
		this.formError = '';
		this.dialogOpen = true;
	}

	async pickFolder() {
		const selectedPath = await selectFolder(this.form.path.trim() || undefined);
		if (!selectedPath) return;
		this.form = { ...this.form, path: selectedPath };
		if (!this.form.name.trim()) {
			this.form = { ...this.form, name: baseName(selectedPath) };
		}
	}

	async save() {
		const nextName = this.form.name.trim();
		const nextPath = this.form.path.trim();
		if (!nextName) {
			this.formError = 'Project name is required.';
			return;
		}
		if (!nextPath) {
			this.formError = 'Select a project folder.';
			return;
		}

		const duplicate = this.projectStore.projects.find((p) => p.path === nextPath);
		if (duplicate && duplicate.path !== this.editingProjectPath) {
			this.formError = 'That folder is already added as a project.';
			return;
		}

		// Spread the saved project so legacy fields (startupCommand, tasks) survive an edit.
		const saved = this.editingProjectPath
			? this.projectStore.getByPath(this.editingProjectPath)
			: undefined;
		const nextProject: ProjectConfig = {
			...saved,
			name: nextName,
			path: nextPath,
			group: this.form.group.trim() || undefined,
			shell: this.form.shell.trim() || undefined
		};

		if (this.dialogMode === 'create') {
			await this.projectStore.add(nextProject);
			this.projectStore.openProject(nextProject.path);
			this.gitStore.refreshGitState(nextProject.path);
		} else {
			const previousPath = this.editingProjectPath;
			if (!previousPath) {
				this.formError = 'Could not determine which project to update.';
				return;
			}
			await this.projectStore.update(previousPath, nextProject);
			this.workspaceStore.updateProjectInfo(previousPath, nextProject.path, nextProject.name);
		}

		this.dialogOpen = false;
		this.resetForm();
	}

	promptNewGroup(projectPath: string) {
		this.groupDialogProjectPath = projectPath;
		this.groupDialogValue = '';
		this.groupDialogOpen = true;
	}

	async saveNewGroup() {
		const name = this.groupDialogValue.trim();
		if (!name || !this.groupDialogProjectPath) return;
		await this.projectStore.setGroup(this.groupDialogProjectPath, name);
		this.groupDialogOpen = false;
		this.groupDialogProjectPath = null;
		this.groupDialogValue = '';
	}

	remove(projectPath: string) {
		this.removal.request(projectPath);
	}

	async confirmRemove() {
		await this.removal.confirm(async (path) => {
			await this.projectStore.removeWithWorkspaces(path);
		});
	}
}
