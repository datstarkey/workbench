import { selectFolder } from '$lib/utils/dialog';
import { ConfirmAction } from '$lib/utils/confirm-action.svelte';
import { baseName } from '$lib/utils/path';
import type { GitStore } from '$stores/git.svelte';
import type { ProjectStore } from '$stores/projects.svelte';
import type { WorkspaceStore } from '$stores/workspaces.svelte';
import type { ProjectConfig, ProjectFormState, ProjectTask } from '$types/workbench';

export class ProjectManagerStore {
	dialogOpen = $state(false);
	dialogMode: 'create' | 'edit' = $state('create');
	form: ProjectFormState = $state({
		name: '',
		path: '',
		shell: '',
		startupCommand: '',
		tasks: []
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
		this.form = { name: '', path: '', shell: '', startupCommand: '', tasks: [] };
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
			shell: project.shell || '',
			startupCommand: project.startupCommand || '',
			tasks: (project.tasks ?? []).map((task) => ({ ...task }))
		};
		this.formError = '';
		this.dialogOpen = true;
	}

	addTask() {
		this.form = {
			...this.form,
			tasks: [...this.form.tasks, { name: '', command: '' }]
		};
	}

	removeTask(index: number) {
		this.form = {
			...this.form,
			tasks: this.form.tasks.filter((_, i) => i !== index)
		};
	}

	updateTaskName(index: number, name: string) {
		this.form.tasks[index].name = name;
	}

	updateTaskCommand(index: number, command: string) {
		this.form.tasks[index].command = command;
	}

	reorderTask(fromIndex: number, toIndex: number) {
		if (fromIndex === toIndex) return;
		const next = [...this.form.tasks];
		const [moved] = next.splice(fromIndex, 1);
		next.splice(toIndex, 0, moved);
		this.form = { ...this.form, tasks: next };
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

		const normalizedTasks = this.form.tasks
			.map((task) => ({ name: task.name.trim(), command: task.command.trim() }))
			.filter((task) => task.name || task.command);
		if (normalizedTasks.some((task) => !task.name || !task.command)) {
			this.formError = 'Each task needs both a name and a command.';
			return;
		}
		if (hasDuplicateTaskNames(normalizedTasks)) {
			this.formError = 'Task names must be unique.';
			return;
		}

		const nextProject: ProjectConfig = {
			name: nextName,
			path: nextPath,
			shell: this.form.shell.trim() || undefined,
			startupCommand: this.form.startupCommand.trim() || undefined,
			tasks: normalizedTasks.length > 0 ? normalizedTasks : undefined
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

	remove(projectPath: string) {
		this.removal.request(projectPath);
	}

	async confirmRemove() {
		await this.removal.confirm(async (path) => {
			await this.projectStore.removeWithWorkspaces(path);
		});
	}
}

function hasDuplicateTaskNames(tasks: ProjectTask[]): boolean {
	const seen: Record<string, true> = {};
	for (const task of tasks) {
		const key = task.name.toLowerCase();
		if (seen[key]) return true;
		seen[key] = true;
	}
	return false;
}
