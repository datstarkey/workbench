<script lang="ts">
	import GitForkIcon from '@lucide/svelte/icons/git-fork';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import SearchIcon from '@lucide/svelte/icons/search';
	import { invoke } from '@tauri-apps/api/core';
	import { onMount } from 'svelte';
	import { toast } from 'svelte-sonner';
	import { getGitStore, getWorktreeManager } from '$stores/context';
	import type { BranchInfo } from '$types/workbench';
	import { branchOptions, type BranchOption } from './git-view';

	let {
		path,
		projectPath,
		currentBranch,
		onDone
	}: { path: string; projectPath: string; currentBranch: string; onDone: () => void } = $props();

	const gitStore = getGitStore();
	const worktreeManager = getWorktreeManager();

	type Entry = { kind: 'branch'; option: BranchOption } | { kind: 'create'; name: string };

	let branches = $state.raw<BranchInfo[]>([]);
	let query = $state('');
	let worktreeBranches = $derived(
		(gitStore.worktreesByProject[projectPath] ?? []).map((w) => w.branch).filter(Boolean)
	);
	let options = $derived(branchOptions(branches, worktreeBranches, query));
	let entries = $derived<Entry[]>([
		...options.local
			.filter((o) => !o.inWorktree)
			.map((option) => ({ kind: 'branch' as const, option })),
		...options.remote.map((option) => ({ kind: 'branch' as const, option })),
		...(options.createName ? [{ kind: 'create' as const, name: options.createName }] : [])
	]);
	// Writable derived: resets the highlight whenever the filter changes
	let active = $derived.by(() => {
		void query;
		return 0;
	});

	onMount(async () => {
		try {
			branches = await invoke<BranchInfo[]>('list_branches', { path });
		} catch (e) {
			toast.error(`Failed to list branches: ${e}`);
		}
	});

	function indexOf(option: BranchOption): number {
		return entries.findIndex((e) => e.kind === 'branch' && e.option === option);
	}

	async function select(entry: Entry) {
		onDone();
		try {
			if (entry.kind === 'create') {
				await gitStore.createBranch(path, entry.name, true);
				toast.success(`Created and switched to ${entry.name}`);
			} else {
				await gitStore.checkoutBranch(path, entry.option.checkoutName);
				toast.success(`Switched to ${entry.option.checkoutName}`);
			}
		} catch (e) {
			toast.error(String(e));
		}
	}

	function onkeydown(e: KeyboardEvent) {
		if (e.key === 'ArrowDown') active = Math.min(active + 1, entries.length - 1);
		else if (e.key === 'ArrowUp') active = Math.max(active - 1, 0);
		else if (e.key === 'Enter' && entries[active]) void select(entries[active]);
		else return;
		e.preventDefault();
	}

	function newWorktree() {
		onDone();
		worktreeManager.add(projectPath, { suggestedBranch: options.createName ?? undefined });
	}
</script>

{#snippet row(option: BranchOption)}
	{@const index = indexOf(option)}
	<button
		type="button"
		class={[
			'mx-1 flex h-[30px] items-center gap-2 rounded-[5px] px-2 text-left text-xs',
			option.inWorktree ? 'cursor-not-allowed text-wb-ink-soft' : 'text-wb-ink',
			index === active && 'bg-wb-accent-soft'
		]}
		disabled={option.inWorktree}
		title={option.inWorktree ? 'Checked out in another worktree' : undefined}
		onmouseenter={() => index >= 0 && (active = index)}
		onclick={() => select({ kind: 'branch', option })}
	>
		<span class="min-w-0 flex-1 truncate font-mono">{option.name}</span>
		<span class="shrink-0 text-[10.5px] text-wb-ink-soft">
			{option.inWorktree ? 'in worktree' : option.sha}
		</span>
	</button>
{/snippet}

{#snippet heading(text: string)}
	<div
		class="px-2.5 pt-2 pb-0.5 text-[10px] font-semibold tracking-[0.06em] text-wb-ink-soft uppercase"
	>
		{text}
	</div>
{/snippet}

<div class="flex flex-col">
	<div class="flex h-9 items-center gap-2 border-b border-wb-hair px-2.5">
		<SearchIcon class="size-3.5 shrink-0 text-wb-ink-soft" />
		<label for="branch-filter" class="sr-only">Find or create a branch</label>
		<!-- The popover focuses its first tabbable element (this input) on open -->
		<input
			id="branch-filter"
			class="h-7 min-w-0 flex-1 bg-transparent font-mono text-xs text-wb-ink outline-none placeholder:font-sans placeholder:text-wb-ink-soft"
			placeholder="Find or create a branch…"
			autocomplete="off"
			spellcheck="false"
			bind:value={query}
			{onkeydown}
		/>
	</div>

	<div class="flex max-h-72 flex-col overflow-y-auto pb-1">
		{#if options.local.length > 0}
			{@render heading('Local')}
			{#each options.local as option (option.name)}
				{@render row(option)}
			{/each}
		{/if}
		{#if options.remote.length > 0}
			{@render heading('Remote')}
			{#each options.remote as option (option.name)}
				{@render row(option)}
			{/each}
		{/if}
		{#if options.local.length === 0 && options.remote.length === 0 && !options.createName}
			<p class="px-2.5 py-3 text-xs text-wb-ink-soft">No other branches</p>
		{/if}
	</div>

	<div class="flex flex-col border-t border-wb-hair p-1">
		{#if options.createName}
			{@const index = entries.length - 1}
			<button
				type="button"
				class={[
					'flex h-8 items-center gap-2 rounded-[5px] px-2 text-left text-xs text-wb-ink',
					index === active && 'bg-wb-accent-soft'
				]}
				onmouseenter={() => (active = index)}
				onclick={() => select({ kind: 'create', name: options.createName! })}
			>
				<PlusIcon class="size-3 shrink-0 text-wb-accent" />
				<span class="min-w-0 flex-1 truncate">
					Create <span class="font-mono text-wb-accent">{options.createName}</span>
				</span>
				<span class="max-w-[45%] shrink-0 truncate text-[10.5px] text-wb-ink-soft">
					from {currentBranch}
				</span>
			</button>
		{/if}
		<button
			type="button"
			class="flex h-8 items-center gap-2 rounded-[5px] px-2 text-left text-xs text-wb-ink-mute hover:bg-wb-hair hover:text-wb-ink"
			onclick={newWorktree}
		>
			<GitForkIcon class="size-3 shrink-0" />
			New worktree…
		</button>
	</div>

	<div class="flex gap-3 border-t border-wb-hair px-2.5 py-1.5 text-[10.5px] text-wb-ink-soft">
		<span>↑↓ navigate</span>
		<span>↵ switch</span>
		<span>esc close</span>
	</div>
</div>
