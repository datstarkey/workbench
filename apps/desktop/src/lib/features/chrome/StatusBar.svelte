<script lang="ts">
	import GitBranchIcon from '@lucide/svelte/icons/git-branch';
	import {
		getClaudeSessionStore,
		getGitHubStore,
		getGitStore,
		getWorkspaceStore
	} from '$stores/context';
	import { getVersion } from '@tauri-apps/api/app';

	const workspaceStore = getWorkspaceStore();
	const gitStore = getGitStore();
	const githubStore = getGitHubStore();
	const sessionStore = getClaudeSessionStore();

	let version = $state('');
	getVersion()
		.then((v) => (version = v))
		.catch(() => {});

	const projectPath = $derived(workspaceStore.activeProjectPath);
	const status = $derived(projectPath ? gitStore.statusByProject[projectPath] : undefined);
	const branch = $derived(
		status?.branch ?? (projectPath ? (gitStore.branchByProject[projectPath] ?? '') : '')
	);
	const ahead = $derived(status?.ahead ?? 0);
	const behind = $derived(status?.behind ?? 0);
	const modified = $derived(status?.files.length ?? 0);

	// CI state for the active workspace branch (via GitHub store's sidebar PR)
	const ci = $derived(githubStore.sidebarPr?.checksStatus.overall ?? 'none');

	// Global active session counts across all projects (derived on the store)
	const sessionCounts = $derived(sessionStore.totalSessionCounts);

	const ciLabel = $derived(
		ci === 'success'
			? 'CI passing'
			: ci === 'failure'
				? 'CI failing'
				: ci === 'pending'
					? 'CI running'
					: ''
	);
	const ciColor = $derived(
		ci === 'success' ? 'var(--wb-ok)' : ci === 'failure' ? 'var(--wb-err)' : 'var(--wb-warn)'
	);
</script>

<footer
	class="flex h-[22px] flex-shrink-0 items-center gap-4 border-t border-wb-hair bg-wb-rail px-2.5 font-mono text-[10.5px] text-wb-ink-mute"
>
	{#if branch}
		<span class="flex items-center gap-1.5 text-wb-accent">
			<GitBranchIcon size={11} />
			{branch}
		</span>
		{#if ahead > 0 || behind > 0}
			<span>↑{ahead} ↓{behind}</span>
		{/if}
		{#if modified > 0}
			<span class="text-wb-warn">● {modified} modified</span>
		{/if}
	{/if}
	{#if ciLabel}
		<span class="flex items-center gap-1.5" style:color={ciColor}>
			<span
				class="h-1.5 w-1.5 rounded-full"
				class:wb-pulse={ci === 'pending'}
				style:background={ciColor}
			></span>
			{ciLabel}
		</span>
	{/if}

	<span class="flex-1"></span>

	{#if sessionCounts.claude > 0}
		<span class="text-wb-claude">● {sessionCounts.claude} Claude</span>
	{/if}
	{#if sessionCounts.codex > 0}
		<span class="text-wb-codex">● {sessionCounts.codex} Codex</span>
	{/if}
	{#if version}
		<span class="text-wb-ink-soft">workbench v{version}</span>
	{/if}
</footer>
