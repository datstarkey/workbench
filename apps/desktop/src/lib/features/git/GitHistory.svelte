<script lang="ts">
	import CopyIcon from '@lucide/svelte/icons/copy';
	import EllipsisIcon from '@lucide/svelte/icons/ellipsis';
	import ExternalLinkIcon from '@lucide/svelte/icons/external-link';
	import UndoIcon from '@lucide/svelte/icons/undo-2';
	import * as DropdownMenu from '@workbench/ui/dropdown-menu';
	import ConfirmDialog from '$components/ConfirmDialog.svelte';
	import SidebarSection from '$features/sidebar/SidebarSection.svelte';
	import { iconButton, textButton } from '$features/sidebar/styles';
	import { commitUrl } from '$features/github/pr-view';
	import { getGitStore } from '$stores/context';
	import { ConfirmAction } from '$lib/utils/confirm-action.svelte';
	import { formatRelativeTime } from '$lib/utils/format';
	import { openInGitHub } from '$lib/utils/github';
	import type { GitCommitFile, GitLogEntry } from '$types/workbench';
	import { toast } from 'svelte-sonner';
	import { getStatusDisplay, splitPath } from './git-view';

	let {
		entries,
		path,
		repoUrl
	}: {
		entries: GitLogEntry[];
		path: string;
		repoUrl: string | null;
	} = $props();

	const PREVIEW_COUNT = 8;
	const gitStore = getGitStore();
	const revert = new ConfirmAction<GitLogEntry>();

	let showAll = $state(false);
	let expandedSha: string | null = $state(null);
	let filesBySha: Record<string, GitCommitFile[]> = $state({});
	let visible = $derived(showAll ? entries : entries.slice(0, PREVIEW_COUNT));

	async function toggle(sha: string) {
		if (expandedSha === sha) {
			expandedSha = null;
			return;
		}
		expandedSha = sha;
		if (!filesBySha[sha]) {
			const files = await gitStore.showFiles(path, sha);
			filesBySha = { ...filesBySha, [sha]: files };
		}
	}

	async function copySha(sha: string) {
		await navigator.clipboard.writeText(sha);
		toast.success(`Copied ${sha.slice(0, 7)}`);
	}

	async function revertCommit(entry: GitLogEntry) {
		const result = await gitStore.revert(path, entry.sha);
		if (result) toast.success(`Reverted: ${result.message}`);
		expandedSha = null;
	}
</script>

<SidebarSection title="History">
	{#each visible as entry, i (entry.sha)}
		{@const isUnpushed = entry.unpushed}
		{@const expanded = expandedSha === entry.sha}
		{@const isLast = i === visible.length - 1}
		<div class={['mx-1 rounded-md', expanded && 'bg-wb-panel2']}>
			<div class="group flex items-start gap-1 pr-1.5">
				<button
					type="button"
					class="flex min-w-0 flex-1 gap-2.5 py-1.5 pl-2.5 text-left"
					aria-expanded={expanded}
					onclick={() => toggle(entry.sha)}
				>
					<span class="flex w-2.5 shrink-0 flex-col items-center self-stretch pt-1">
						<span
							class={[
								'size-2 shrink-0 rounded-full',
								isUnpushed ? 'border-2 border-wb-accent' : 'bg-wb-ink-soft'
							]}
						></span>
						{#if !isLast && !expanded}
							<span class="mt-0.5 -mb-2 w-px flex-1 bg-wb-hair"></span>
						{/if}
					</span>
					<span class="flex min-w-0 flex-1 flex-col gap-0.5">
						<span
							class={[
								'truncate text-xs',
								isUnpushed || expanded ? 'text-wb-ink' : 'text-wb-ink-mute'
							]}
							title={entry.message}
						>
							{entry.message}
						</span>
						<span class="truncate text-[10.5px] text-wb-ink-soft">
							<span class="font-mono">{entry.shortSha}</span>
							· {entry.author} · {formatRelativeTime(entry.date)}
							{#if isUnpushed}
								· <span class="text-wb-accent">not pushed</span>
							{/if}
						</span>
					</span>
				</button>
				<DropdownMenu.Root>
					<DropdownMenu.Trigger
						class={[
							'mt-1.5 opacity-0 group-focus-within:opacity-100 group-hover:opacity-100 data-[state=open]:opacity-100',
							iconButton
						]}
						aria-label="Actions for {entry.shortSha}"
					>
						<EllipsisIcon class="size-3.5" />
					</DropdownMenu.Trigger>
					<DropdownMenu.Content align="end" class="w-44">
						<DropdownMenu.Item onclick={() => copySha(entry.sha)}>
							<CopyIcon class="size-3.5" /> Copy SHA
						</DropdownMenu.Item>
						{#if repoUrl && !isUnpushed}
							<DropdownMenu.Item onclick={() => openInGitHub(commitUrl(repoUrl, entry.sha))}>
								<ExternalLinkIcon class="size-3.5" /> Open on GitHub
							</DropdownMenu.Item>
						{/if}
						<DropdownMenu.Separator />
						<DropdownMenu.Item variant="destructive" onclick={() => revert.request(entry)}>
							<UndoIcon class="size-3.5" /> Revert commit…
						</DropdownMenu.Item>
					</DropdownMenu.Content>
				</DropdownMenu.Root>
			</div>
			{#if expanded}
				<div class="flex flex-col pr-1.5 pb-1.5 pl-[30px]">
					{#each filesBySha[entry.sha] ?? [] as file (file.path)}
						{@const display = getStatusDisplay(file.status)}
						{@const parts = splitPath(file.path)}
						<div class="flex h-[22px] items-center gap-2" title={file.path}>
							<span class="min-w-0 truncate text-xs text-wb-ink">{parts.name}</span>
							<span class="min-w-0 flex-1 truncate text-[11px] text-wb-ink-soft">{parts.dir}</span>
							<span
								class="w-3 shrink-0 text-center font-mono text-[11px] font-semibold {display.color}"
							>
								{display.letter}
							</span>
						</div>
					{:else}
						<p class="py-0.5 text-[11px] text-wb-ink-soft">
							{filesBySha[entry.sha] ? 'No files changed' : 'Loading…'}
						</p>
					{/each}
				</div>
			{/if}
		</div>
	{:else}
		<p class="px-3 py-1 text-xs text-wb-ink-soft">No commits yet</p>
	{/each}
	{#if entries.length > PREVIEW_COUNT}
		<button
			type="button"
			class={['mt-0.5 ml-8 self-start', textButton]}
			onclick={() => (showAll = !showAll)}
		>
			{showAll ? 'Show less' : 'Show more'}
		</button>
	{/if}
</SidebarSection>

<ConfirmDialog
	bind:open={revert.open}
	title="Revert commit?"
	description="Creates a new commit that undoes “{revert.pendingValue?.message ?? ''}”."
	confirmLabel="Revert"
	destructive
	error={revert.error}
	onConfirm={() => revert.confirm(revertCommit)}
/>
