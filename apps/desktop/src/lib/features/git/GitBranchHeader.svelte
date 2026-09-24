<script lang="ts">
	import ArrowDownIcon from '@lucide/svelte/icons/arrow-down';
	import ArrowUpIcon from '@lucide/svelte/icons/arrow-up';
	import CheckIcon from '@lucide/svelte/icons/check';
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import CloudUploadIcon from '@lucide/svelte/icons/cloud-upload';
	import GitBranchIcon from '@lucide/svelte/icons/git-branch';
	import LoaderIcon from '@lucide/svelte/icons/loader';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import { cn } from '@workbench/ui';
	import * as Popover from '@workbench/ui/popover';
	import { primaryButton } from '$features/sidebar/styles';
	import { getGitStore } from '$stores/context';
	import type { GitStatusResult } from '$types/workbench';
	import { toast } from 'svelte-sonner';
	import BranchPicker from './BranchPicker.svelte';
	import { syncButtonLabel, syncState, syncSummary } from './git-view';

	let {
		status,
		path,
		projectPath
	}: { status: GitStatusResult; path: string; projectPath: string } = $props();

	const gitStore = getGitStore();

	const SYNC_ICONS = {
		publish: CloudUploadIcon,
		push: ArrowUpIcon,
		pull: ArrowDownIcon
	};

	let sync = $derived(syncState(status));
	let buttonLabel = $derived(syncButtonLabel(sync));
	let SyncIcon = $derived(
		sync.kind in SYNC_ICONS ? SYNC_ICONS[sync.kind as keyof typeof SYNC_ICONS] : null
	);
	let pickerOpen = $state(false);
	let fetching = $state(false);
	let syncing = $state(false);

	async function fetchRemote() {
		fetching = true;
		try {
			await gitStore.fetch(path);
		} catch (e) {
			toast.error(`Fetch failed: ${e}`);
		} finally {
			fetching = false;
		}
	}

	async function runSync() {
		const kind = sync.kind;
		syncing = true;
		try {
			if (kind === 'pull') await gitStore.pull(path);
			else await gitStore.push(path, kind === 'publish');
			toast.success(kind === 'publish' ? 'Branch published' : 'Synced with remote');
		} catch (e) {
			toast.error(`${buttonLabel} failed: ${e}`);
		} finally {
			syncing = false;
		}
	}
</script>

<div class="flex flex-col gap-2.5 border-b border-wb-hair-soft px-3 pt-3 pb-3.5">
	<div class="flex items-center gap-1.5">
		<Popover.Root bind:open={pickerOpen}>
			<Popover.Trigger
				class="flex h-7 min-w-0 flex-1 items-center gap-1.5 rounded-md border border-wb-hair bg-wb-bg px-2 font-mono text-[12.5px] font-semibold text-wb-ink hover:border-wb-ink-soft data-[state=open]:border-wb-accent"
				aria-label="Switch branch, current {status.branch}"
			>
				<GitBranchIcon class="size-3.5 shrink-0 text-wb-accent" />
				<span class="min-w-0 flex-1 truncate text-left">{status.branch}</span>
				<ChevronDownIcon class="size-3 shrink-0 text-wb-ink-mute" />
			</Popover.Trigger>
			<Popover.Content
				align="start"
				class="w-80 overflow-hidden border-wb-hair bg-wb-panel2 p-0 text-wb-ink"
			>
				<BranchPicker
					{path}
					{projectPath}
					currentBranch={status.branch}
					onDone={() => (pickerOpen = false)}
				/>
			</Popover.Content>
		</Popover.Root>
		<button
			type="button"
			class="flex size-7 shrink-0 items-center justify-center rounded-md border border-wb-hair text-wb-ink-mute hover:bg-wb-panel2 hover:text-wb-ink disabled:opacity-50"
			aria-label="Fetch from remote"
			title="Fetch from remote"
			disabled={fetching}
			onclick={fetchRemote}
		>
			<RefreshCwIcon class={['size-3.5', fetching && 'animate-spin']} />
		</button>
	</div>

	<div class="flex min-h-[26px] items-center gap-2">
		{#if sync.kind === 'synced'}
			<CheckIcon class="size-3 shrink-0 text-wb-ok" />
		{/if}
		<span
			class={['flex-1 text-[11px]', sync.kind === 'diverged' ? 'text-wb-warn' : 'text-wb-ink-mute']}
			title={sync.kind === 'diverged'
				? 'Rebase or merge the remote changes in a terminal, then push'
				: undefined}
		>
			{syncSummary(sync)}
		</span>
		{#if buttonLabel}
			<button
				type="button"
				class={cn(primaryButton, 'h-[26px] rounded-md px-2.5 text-[11.5px]')}
				disabled={syncing}
				onclick={runSync}
			>
				{#if syncing}
					<LoaderIcon class="size-3 animate-spin" />
				{:else if SyncIcon}
					<SyncIcon class="size-3" />
				{/if}
				{buttonLabel}
			</button>
		{/if}
	</div>
</div>
