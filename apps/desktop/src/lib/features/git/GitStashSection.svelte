<script lang="ts">
	import TrashIcon from '@lucide/svelte/icons/trash-2';
	import ConfirmDialog from '$components/ConfirmDialog.svelte';
	import SidebarSection from '$features/sidebar/SidebarSection.svelte';
	import { iconButton, outlineButton } from '$features/sidebar/styles';
	import { getGitStore } from '$stores/context';
	import { ConfirmAction } from '$lib/utils/confirm-action.svelte';
	import { formatRelativeTime } from '$lib/utils/format';
	import type { GitStashEntry } from '$types/workbench';
	import { toast } from 'svelte-sonner';

	let { stashes, path }: { stashes: GitStashEntry[]; path: string } = $props();

	const gitStore = getGitStore();
	const drop = new ConfirmAction<GitStashEntry>();

	async function pop(index: number) {
		try {
			await gitStore.stashPop(path, index);
			toast.success('Stash applied and removed');
		} catch (e) {
			toast.error(`Failed to pop stash: ${e}`);
		}
	}
</script>

<SidebarSection title="Stashes" count={stashes.length} open={false}>
	{#each stashes as stash (stash.index)}
		<div
			class="group mx-1 flex items-center gap-2 rounded-md py-1.5 pr-1.5 pl-2.5 hover:bg-wb-panel2"
		>
			<div class="flex min-w-0 flex-1 flex-col gap-0.5">
				<span class="truncate text-xs text-wb-ink" title={stash.message}>
					{stash.message || `stash@{${stash.index}}`}
				</span>
				<span class="text-[10.5px] text-wb-ink-soft">
					<span class="font-mono">{`stash@{${stash.index}}`}</span> · {formatRelativeTime(
						stash.date
					)}
				</span>
			</div>
			<button
				type="button"
				class={outlineButton}
				title="Apply these changes and remove the stash"
				onclick={() => pop(stash.index)}
			>
				Pop
			</button>
			<button
				type="button"
				class={iconButton}
				aria-label={`Drop stash@{${stash.index}}`}
				title="Drop stash…"
				onclick={() => drop.request(stash)}
			>
				<TrashIcon class="size-3" />
			</button>
		</div>
	{/each}
</SidebarSection>

<ConfirmDialog
	bind:open={drop.open}
	title="Drop stash?"
	description="“{drop.pendingValue?.message ?? ''}” is deleted for good."
	confirmLabel="Drop"
	destructive
	error={drop.error}
	onConfirm={() => drop.confirm((s) => gitStore.stashDrop(path, s.index))}
/>
