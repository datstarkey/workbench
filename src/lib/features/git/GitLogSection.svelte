<script lang="ts">
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import CopyIcon from '@lucide/svelte/icons/copy';
	import UndoIcon from '@lucide/svelte/icons/undo-2';
	import { Button } from '$lib/components/ui/button';
	import { getGitStore } from '$stores/context';
	import type { GitCommitFile, GitLogEntry } from '$types/workbench';
	import { toast } from 'svelte-sonner';
	import { getStatusDisplay } from './status-display';

	let {
		entries,
		path
	}: {
		entries: GitLogEntry[];
		path: string;
	} = $props();

	const gitStore = getGitStore();

	let expanded = $state(true);
	let expandedSha: string | null = $state(null);
	let filesBySha: Record<string, GitCommitFile[]> = $state({});
	let loadingSha: string | null = $state(null);

	async function toggleExpand(sha: string) {
		if (expandedSha === sha) {
			expandedSha = null;
			return;
		}
		expandedSha = sha;
		if (!filesBySha[sha]) {
			loadingSha = sha;
			const files = await gitStore.showFiles(path, sha);
			filesBySha = { ...filesBySha, [sha]: files };
			loadingSha = null;
		}
	}

	async function copySha(sha: string) {
		await navigator.clipboard.writeText(sha);
		toast.success(`Copied ${sha.slice(0, 7)}`);
	}

	async function revertCommit(sha: string) {
		try {
			const result = await gitStore.revert(path, sha);
			if (result) toast.success(`Reverted: ${result.message}`);
			expandedSha = null;
		} catch (e) {
			toast.error(`Revert failed: ${e}`);
		}
	}
</script>

<div>
	<button
		type="button"
		class="flex w-full items-center gap-1 px-2 py-1 text-xs font-medium text-muted-foreground hover:text-foreground"
		onclick={() => (expanded = !expanded)}
	>
		{#if expanded}
			<ChevronDownIcon class="size-3" />
		{:else}
			<ChevronRightIcon class="size-3" />
		{/if}
		Recent Commits
	</button>
	{#if expanded}
		<div class="space-y-0.5 px-1">
			{#each entries as entry (entry.sha)}
				<div>
					<button
						type="button"
						class="group flex w-full items-center gap-1.5 rounded px-1.5 py-0.5 text-left hover:bg-muted/40"
						onclick={() => toggleExpand(entry.sha)}
					>
						<span class="shrink-0 font-mono text-[10px] text-muted-foreground"
							>{entry.shortSha}</span
						>
						<span
							class="min-w-0 flex-1 truncate text-xs text-foreground/80"
							title={entry.message}>{entry.message}</span
						>
						<span
							class="flex shrink-0 items-center gap-0.5 opacity-0 group-hover:opacity-100"
						>
							<Button
								variant="ghost"
								size="icon-sm"
								class="size-5"
								onclick={(e: MouseEvent) => {
									e.stopPropagation();
									copySha(entry.sha);
								}}
								title="Copy full SHA"
							>
								<CopyIcon class="size-3" />
							</Button>
							<Button
								variant="ghost"
								size="icon-sm"
								class="size-5"
								onclick={(e: MouseEvent) => {
									e.stopPropagation();
									revertCommit(entry.sha);
								}}
								title="Revert this commit"
							>
								<UndoIcon class="size-3" />
							</Button>
						</span>
					</button>
					{#if expandedSha === entry.sha}
						<div class="ml-5 space-y-0.5 border-l border-muted py-0.5 pl-2">
							{#if loadingSha === entry.sha}
								<p class="text-[10px] text-muted-foreground">Loading...</p>
							{:else if filesBySha[entry.sha]}
								{#each filesBySha[entry.sha] as file (file.path)}
									{@const display = getStatusDisplay(file.status)}
									<div class="flex items-center gap-1.5 px-1 py-0.5">
										<span
											class="w-3 shrink-0 text-center font-mono text-[10px] font-medium {display.color}"
										>
											{display.letter}
										</span>
										<span
											class="min-w-0 truncate font-mono text-[10px] text-foreground/70"
											title={file.path}
										>
											{file.path}
										</span>
									</div>
								{/each}
								{#if filesBySha[entry.sha].length === 0}
									<p class="text-[10px] text-muted-foreground">No files changed</p>
								{/if}
							{/if}
						</div>
					{/if}
				</div>
			{/each}
			{#if entries.length === 0}
				<p class="px-1.5 py-1 text-xs text-muted-foreground">No commits yet</p>
			{/if}
		</div>
	{/if}
</div>
