import { createContext } from 'svelte';
import type { ProjectStore } from './projects.svelte';
import type { WorkspaceStore } from './workspaces.svelte';
import type { ClaudeSessionStore } from './claudeSessions.svelte';
import type { ClaudeSettingsStore } from './claude-settings.svelte';
import type { GitStore } from './git.svelte';
import type { ProjectManagerStore } from '$features/projects/project-manager.svelte';
import type { GitHubStore } from './github.svelte';
import type { WorktreeManagerStore } from '$features/worktrees/worktree-manager.svelte';

export const [getProjectStore, setProjectStore] = createContext<ProjectStore>();
export const [getWorkspaceStore, setWorkspaceStore] = createContext<WorkspaceStore>();
export const [getClaudeSessionStore, setClaudeSessionStore] = createContext<ClaudeSessionStore>();
export const [getClaudeSettingsStore, setClaudeSettingsStore] =
	createContext<ClaudeSettingsStore>();
export const [getGitStore, setGitStore] = createContext<GitStore>();
export const [getProjectManager, setProjectManager] = createContext<ProjectManagerStore>();
export const [getWorktreeManager, setWorktreeManager] = createContext<WorktreeManagerStore>();
export const [getGitHubStore, setGitHubStore] = createContext<GitHubStore>();
