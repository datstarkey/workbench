<script lang="ts">
	import ConfirmDialog from '$components/ConfirmDialog.svelte';
	import SidebarSection from '$features/sidebar/SidebarSection.svelte';
	import { textButton } from '$features/sidebar/styles';
	import { getGitStore } from '$stores/context';
	import { ConfirmAction } from '$lib/utils/confirm-action.svelte';
	import type { GitFileStatus } from '$types/workbench';
	import FileItem from './FileItem.svelte';
	import { splitPath, stagedFiles, unstagedFiles } from './git-view';

	let { files, path }: { files: GitFileStatus[]; path: string } = $props();

	const gitStore = getGitStore();
	const discard = new ConfirmAction<GitFileStatus>();

	let staged = $derived(stagedFiles(files));
	let unstaged = $derived(unstagedFiles(files));
	let discardName = $derived(discard.pendingValue ? splitPath(discard.pendingValue.path).name : '');
</script>

{#if staged.length > 0}
	<SidebarSection title="Staged" count={staged.length}>
		{#snippet actions()}
			<button
				type="button"
				class={textButton}
				onclick={() =>
					gitStore.unstageFiles(
						path,
						staged.map((f) => f.path)
					)}
			>
				Unstage all
			</button>
		{/snippet}
		{#each staged as file (file.path)}
			<FileItem {file} staged onToggle={() => gitStore.unstageFiles(path, [file.path])} />
		{/each}
	</SidebarSection>
{/if}

{#if unstaged.length > 0}
	<SidebarSection title="Changes" count={unstaged.length}>
		{#snippet actions()}
			<button
				type="button"
				class={textButton}
				onclick={() =>
					gitStore.stageFiles(
						path,
						unstaged.map((f) => f.path)
					)}
			>
				Stage all
			</button>
		{/snippet}
		{#each unstaged as file (file.path)}
			<FileItem
				{file}
				staged={false}
				onToggle={() => gitStore.stageFiles(path, [file.path])}
				onDiscard={file.status === 'untracked' ? undefined : () => discard.request(file)}
			/>
		{/each}
	</SidebarSection>
{/if}

<ConfirmDialog
	bind:open={discard.open}
	title="Discard changes?"
	description="{discardName} goes back to its last committed state. This can't be undone."
	confirmLabel="Discard"
	destructive
	error={discard.error}
	onConfirm={() => discard.confirm((f) => gitStore.discardFile(path, f.path))}
/>
