import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { invokeSpy, mockInvoke, clearInvokeMocks } from '../../test/tauri-mocks';
import { ProjectStore } from './projects.svelte';
import type { WorkspaceStore } from './workspaces.svelte';
import type { ProjectConfig } from '$types/workbench';

function makeProject(overrides: Partial<ProjectConfig> = {}): ProjectConfig {
	return { name: 'Test', path: '/projects/test', ...overrides };
}

describe('ProjectStore', () => {
	let store: ProjectStore;
	const mockWorkspaceStore = {
		open: vi.fn(),
		closeAllForProject: vi.fn(),
		updateProject: vi.fn()
	} as unknown as WorkspaceStore;

	beforeEach(() => {
		store = new ProjectStore(mockWorkspaceStore);
		vi.mocked(mockWorkspaceStore.open).mockReset();
		vi.mocked(mockWorkspaceStore.closeAllForProject).mockReset();
		vi.mocked(mockWorkspaceStore.updateProject).mockReset();
	});

	afterEach(() => {
		clearInvokeMocks();
	});

	describe('load', () => {
		it('populates projects from backend', async () => {
			const projects: ProjectConfig[] = [
				makeProject({ name: 'A', path: '/a' }),
				makeProject({ name: 'B', path: '/b' })
			];
			mockInvoke('list_projects', () => projects);

			await store.load();

			expect(invokeSpy).toHaveBeenCalledWith('list_projects');
			expect(store.projects).toEqual(projects);
			expect(store.loaded).toBe(true);
		});

		it('starts with loaded = false', () => {
			expect(store.loaded).toBe(false);
		});
	});

	describe('persist', () => {
		it('saves current projects to backend', async () => {
			const projects = [makeProject({ name: 'A', path: '/a' })];
			store.projects = projects;

			await store.persist();

			expect(invokeSpy).toHaveBeenCalledWith('save_projects', { projects });
		});
	});

	describe('getByPath', () => {
		it('returns matching project', () => {
			store.projects = [makeProject({ path: '/a' }), makeProject({ path: '/b' })];
			expect(store.getByPath('/b')).toEqual(makeProject({ path: '/b' }));
		});

		it('returns undefined when not found', () => {
			store.projects = [makeProject({ path: '/a' })];
			expect(store.getByPath('/z')).toBeUndefined();
		});
	});

	describe('add', () => {
		it('appends project and persists', async () => {
			const existing = makeProject({ name: 'Existing', path: '/existing' });
			store.projects = [existing];

			const newProject = makeProject({ name: 'New', path: '/new' });
			await store.add(newProject);

			expect(store.projects).toEqual([existing, newProject]);
			expect(invokeSpy).toHaveBeenCalledWith('save_projects', {
				projects: [existing, newProject]
			});
		});
	});

	describe('update', () => {
		it('replaces matching project by previous path and persists', async () => {
			store.projects = [
				makeProject({ name: 'A', path: '/a' }),
				makeProject({ name: 'B', path: '/b' })
			];

			const updated = makeProject({ name: 'A-updated', path: '/a-new' });
			await store.update('/a', updated);

			expect(store.projects[0]).toEqual(updated);
			expect(store.projects[1]).toEqual(makeProject({ name: 'B', path: '/b' }));
			expect(invokeSpy).toHaveBeenCalledWith('save_projects', {
				projects: [updated, makeProject({ name: 'B', path: '/b' })]
			});
		});

		it("moves the project's open workspaces along", async () => {
			store.projects = [makeProject({ name: 'A', path: '/a' })];
			const moved = makeProject({ name: 'A2', path: '/a2' });
			await store.update('/a', moved);
			expect(mockWorkspaceStore.updateProject).toHaveBeenCalledExactlyOnceWith('/a', moved);

			await store.update('/a2', { ...moved, group: 'work' });
			expect(mockWorkspaceStore.updateProject).toHaveBeenCalledOnce();
		});

		it('does not change anything if path not found', async () => {
			const original = [makeProject({ name: 'A', path: '/a' })];
			store.projects = [...original];

			await store.update('/nonexistent', makeProject({ name: 'X', path: '/x' }));

			expect(store.projects).toEqual(original);
			expect(mockWorkspaceStore.updateProject).not.toHaveBeenCalled();
		});
	});

	describe('remove', () => {
		it('filters out project by path and persists', async () => {
			store.projects = [
				makeProject({ name: 'A', path: '/a' }),
				makeProject({ name: 'B', path: '/b' })
			];

			await store.remove('/a');

			expect(store.projects).toEqual([makeProject({ name: 'B', path: '/b' })]);
			expect(invokeSpy).toHaveBeenCalledWith('save_projects', {
				projects: [makeProject({ name: 'B', path: '/b' })]
			});
		});
	});

	describe('reorder', () => {
		it('moves a project from one position to another', () => {
			store.projects = [
				makeProject({ name: 'A', path: '/a' }),
				makeProject({ name: 'B', path: '/b' }),
				makeProject({ name: 'C', path: '/c' })
			];

			store.reorder('/a', '/c');

			expect(store.projects.map((p) => p.path)).toEqual(['/b', '/c', '/a']);
		});

		it('does nothing when fromPath not found', () => {
			store.projects = [makeProject({ name: 'A', path: '/a' })];
			store.reorder('/z', '/a');
			expect(store.projects).toEqual([makeProject({ name: 'A', path: '/a' })]);
		});

		it('does nothing when toPath not found', () => {
			store.projects = [makeProject({ name: 'A', path: '/a' })];
			store.reorder('/a', '/z');
			expect(store.projects).toEqual([makeProject({ name: 'A', path: '/a' })]);
		});

		it('does nothing when from and to are the same', () => {
			store.projects = [makeProject({ name: 'A', path: '/a' })];
			store.reorder('/a', '/a');
			expect(store.projects).toEqual([makeProject({ name: 'A', path: '/a' })]);
		});

		it('calls persist after reordering', () => {
			store.projects = [
				makeProject({ name: 'A', path: '/a' }),
				makeProject({ name: 'B', path: '/b' })
			];
			store.reorder('/a', '/b');
			expect(invokeSpy).toHaveBeenCalledWith('save_projects', expect.any(Object));
		});
	});

	describe('openProject', () => {
		it('calls workspaces.open with the matching project', () => {
			const project = makeProject({ name: 'A', path: '/a' });
			store.projects = [project];

			store.openProject('/a');

			expect(mockWorkspaceStore.open).toHaveBeenCalledWith(project);
		});

		it('does nothing if project not found', () => {
			store.projects = [];
			store.openProject('/nonexistent');
			expect(mockWorkspaceStore.open).not.toHaveBeenCalled();
		});
	});

	describe('removeWithWorkspaces', () => {
		it('closes workspaces then removes project', async () => {
			store.projects = [makeProject({ name: 'A', path: '/a' })];

			await store.removeWithWorkspaces('/a');

			expect(mockWorkspaceStore.closeAllForProject).toHaveBeenCalledWith('/a');
			expect(store.projects).toEqual([]);
			expect(invokeSpy).toHaveBeenCalledWith('save_projects', { projects: [] });
		});
	});

	describe('favourites', () => {
		beforeEach(() => {
			const mem: Record<string, string> = {};
			vi.stubGlobal('localStorage', {
				getItem: (k: string) => mem[k] ?? null,
				setItem: (k: string, v: string) => void (mem[k] = v)
			});
			store = new ProjectStore(mockWorkspaceStore);
			mockInvoke('save_projects', () => undefined);
		});

		afterEach(() => {
			vi.unstubAllGlobals();
		});

		it('toggles a favourite and remembers it across stores', () => {
			store.toggleFavourite('/a');
			expect(store.favourites.has('/a')).toBe(true);
			expect(new ProjectStore(mockWorkspaceStore).favourites.has('/a')).toBe(true);

			store.toggleFavourite('/a');
			expect(new ProjectStore(mockWorkspaceStore).favourites.has('/a')).toBe(false);
		});

		it('reopens a collapsed Favourites section for a new favourite', () => {
			store.toggleSection('favourites');
			store.toggleFavourite('/a');
			expect(store.collapsedSections.has('favourites')).toBe(false);

			store.toggleSection('favourites');
			store.toggleFavourite('/a');
			expect(store.collapsedSections.has('favourites')).toBe(true);
		});

		it('ignores unreadable saved favourites', () => {
			localStorage.setItem('workbench.favourites', '{"not":"an array"}');
			expect([...new ProjectStore(mockWorkspaceStore).favourites]).toEqual([]);
		});

		it('follows a project whose path changes and drops a removed one', async () => {
			store.projects = [makeProject({ path: '/a' })];
			store.toggleFavourite('/a');

			await store.update('/a', makeProject({ path: '/b' }));
			expect([...store.favourites]).toEqual(['/b']);

			await store.remove('/b');
			expect(new ProjectStore(mockWorkspaceStore).favourites.size).toBe(0);
		});
	});
});
