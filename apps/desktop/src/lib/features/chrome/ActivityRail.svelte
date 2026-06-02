<script lang="ts">
	import BookOpenIcon from '@lucide/svelte/icons/book-open';
	import FolderIcon from '@lucide/svelte/icons/folder';
	import GitBranchIcon from '@lucide/svelte/icons/git-branch';
	import GithubIcon from '@lucide/svelte/icons/github';
	import ServerIcon from '@lucide/svelte/icons/server';
	import SettingsIcon from '@lucide/svelte/icons/settings';
	import SparklesIcon from '@lucide/svelte/icons/sparkles';
	import * as Tooltip from '@workbench/ui/tooltip';
	import {
		getGitHubStore,
		getGitStore,
		getProjectStore,
		getWorkspaceStore,
		getWorktreeManager
	} from '$stores/context';

	let {
		onToggleSidebar,
		onOpenSettings,
		onOpenRemote
	}: { onToggleSidebar: () => void; onOpenSettings: () => void; onOpenRemote: () => void } =
		$props();

	const projectStore = getProjectStore();
	const workspaceStore = getWorkspaceStore();
	const gitStore = getGitStore();
	const githubStore = getGitHubStore();
	const worktreeManager = getWorktreeManager();

	const projectPath = $derived(workspaceStore.activeProjectPath);
	const projectCount = $derived(projectStore.projects.length);
	const worktreeCount = $derived(
		projectPath ? (gitStore.worktreesByProject[projectPath]?.length ?? 0) : 0
	);
	const openPrCount = $derived(
		projectPath
			? (githubStore.prsByProject[projectPath]?.filter((p) => p.state === 'OPEN').length ?? 0)
			: 0
	);

	function newWorktree() {
		if (projectPath) void worktreeManager.add(projectPath);
	}
</script>

{#snippet railButton(
	label: string,
	active: boolean,
	badge: number | undefined,
	onclick: () => void,
	icon: typeof FolderIcon
)}
	{@const Icon = icon}
	<Tooltip.Root>
		<Tooltip.Trigger
			{onclick}
			class="relative grid h-9 w-9 place-items-center rounded-md transition-colors {active
				? 'bg-wb-panel2 text-wb-accent'
				: 'text-wb-ink-mute hover:text-wb-ink'}"
		>
			<Icon size={16} />
			{#if badge}
				<span
					class="absolute top-1 right-1 grid h-3 min-w-3 place-items-center rounded-full bg-wb-claude px-[3px] font-mono text-[9px] font-semibold text-white"
				>
					{badge}
				</span>
			{/if}
			{#if active}
				<span class="absolute top-1.5 bottom-1.5 -left-px w-0.5 rounded-sm bg-wb-accent"></span>
			{/if}
		</Tooltip.Trigger>
		<Tooltip.Content side="right">{label}</Tooltip.Content>
	</Tooltip.Root>
{/snippet}

<nav
	class="flex w-11 flex-shrink-0 flex-col items-center gap-0.5 border-r border-wb-hair bg-wb-rail py-2"
>
	{@render railButton('Projects', true, projectCount || undefined, onToggleSidebar, FolderIcon)}
	{@render railButton(
		'New worktree',
		false,
		worktreeCount || undefined,
		newWorktree,
		GitBranchIcon
	)}
	{@render railButton(
		'GitHub',
		githubStore.sidebarOpen,
		openPrCount || undefined,
		() => void githubStore.toggleSidebar(),
		GithubIcon
	)}
	{@render railButton('Sessions', false, undefined, () => {}, SparklesIcon)}
	{@render railButton('Remote server', false, undefined, onOpenRemote, ServerIcon)}
	{@render railButton('Docs', false, undefined, () => {}, BookOpenIcon)}
	{@render railButton('Settings', false, undefined, onOpenSettings, SettingsIcon)}

	<div class="flex-1"></div>
	<div
		class="mb-1 grid h-7 w-7 place-items-center rounded-full bg-wb-panel2 text-[11px] font-semibold text-wb-accent"
	>
		AJ
	</div>
</nav>
