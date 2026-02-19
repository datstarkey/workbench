<script lang="ts">
	import ArrowDownIcon from '@lucide/svelte/icons/arrow-down';
	import ArrowUpIcon from '@lucide/svelte/icons/arrow-up';
	import CheckIcon from '@lucide/svelte/icons/check';
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { getGitStore } from '$stores/context';
	import type { GitStatusResult } from '$types/workbench';
	import { toast } from 'svelte-sonner';
	import FileItem from './FileItem.svelte';

	let {
		status,
		path
	}: {
		status: GitStatusResult;
		path: string;
	} = $props();

	const gitStore = getGitStore();

	let commitMessage = $state('');
	let committing = $state(false);
	let fetching = $state(false);
	let pulling = $state(false);
	let pushing = $state(false);
	let stagedExpanded = $state(true);
	let unstagedExpanded = $state(true);

	let stagedFiles = $derived(status.files.filter((f) => f.staged));
	let unstagedFiles = $derived(status.files.filter((f) => !f.staged));
	let isClean = $derived(status.files.length === 0);

	async function handleCommit() {
		if (!commitMessage.trim() || stagedFiles.length === 0) return;
		committing = true;
		try {
			const result = await gitStore.commit(path, commitMessage.trim());
			if (result) {
				commitMessage = '';
				toast.success(`Committed ${result.sha.slice(0, 7)}`);
			}
		} finally {
			committing = false;
		}
	}

	async function handleFetch() {
		fetching = true;
		try {
			await gitStore.fetch(path);
		} catch (e) {
			toast.error(`Fetch failed: ${e}`);
		} finally {
			fetching = false;
		}
	}

	async function handlePull() {
		pulling = true;
		try {
			await gitStore.pull(path);
			toast.success('Pulled successfully');
		} catch (e) {
			toast.error(`Pull failed: ${e}`);
		} finally {
			pulling = false;
		}
	}

	async function handlePush() {
		pushing = true;
		try {
			await gitStore.push(path, !status.hasUpstream);
			toast.success('Pushed successfully');
		} catch (e) {
			toast.error(`Push failed: ${e}`);
		} finally {
			pushing = false;
		}
	}
</script>

<div>
	<!-- Branch header + sync actions -->
	<div class="flex items-center gap-1.5 px-2 py-1">
		<span class="text-xs font-medium">{status.branch}</span>
		{#if status.hasUpstream}
			{#if status.ahead > 0}
				<Badge variant="secondary" class="h-4 px-1 text-[10px]">{status.ahead}↑</Badge>
			{/if}
			{#if status.behind > 0}
				<Badge variant="secondary" class="h-4 px-1 text-[10px]">{status.behind}↓</Badge>
			{/if}
			{#if status.ahead === 0 && status.behind === 0}
				<span class="text-[10px] text-muted-foreground">in sync</span>
			{/if}
		{:else}
			<span class="text-[10px] text-muted-foreground">no upstream</span>
		{/if}

		<div class="ml-auto flex items-center gap-0.5">
			<Button
				variant="ghost"
				size="icon-sm"
				class="size-5"
				disabled={fetching}
				onclick={handleFetch}
				title="Fetch from remote"
			>
				<RefreshCwIcon class="size-3 {fetching ? 'animate-spin' : ''}" />
			</Button>
			{#if status.hasUpstream && status.behind > 0}
				<Button
					variant="ghost"
					size="icon-sm"
					class="size-5"
					disabled={pulling}
					onclick={handlePull}
					title="Pull {status.behind} commit{status.behind === 1 ? '' : 's'}"
				>
					<ArrowDownIcon class="size-3" />
				</Button>
			{/if}
			{#if status.ahead > 0 || !status.hasUpstream}
				<Button
					variant="ghost"
					size="icon-sm"
					class="size-5"
					disabled={pushing}
					onclick={handlePush}
					title={status.hasUpstream
						? `Push ${status.ahead} commit${status.ahead === 1 ? '' : 's'}`
						: 'Publish branch'}
				>
					<ArrowUpIcon class="size-3" />
				</Button>
			{/if}
		</div>
	</div>

	{#if isClean}
		<p class="px-3 py-2 text-xs text-muted-foreground">Working tree clean</p>
	{:else}
		<!-- Staged files -->
		{#if stagedFiles.length > 0}
			<div>
				<button
					type="button"
					class="flex w-full items-center gap-1 px-2 py-1 text-xs font-medium text-muted-foreground hover:text-foreground"
					onclick={() => (stagedExpanded = !stagedExpanded)}
				>
					{#if stagedExpanded}
						<ChevronDownIcon class="size-3" />
					{:else}
						<ChevronRightIcon class="size-3" />
					{/if}
					Staged
					<Badge variant="secondary" class="ml-auto h-4 px-1 text-[10px]">
						{stagedFiles.length}
					</Badge>
				</button>
				{#if stagedExpanded}
					<div class="space-y-0.5 px-1">
						{#each stagedFiles as file (file.path + ':staged')}
							<FileItem
								{file}
								actionIcon="minus"
								onAction={() => gitStore.unstageFiles(path, [file.path])}
							/>
						{/each}
					</div>
				{/if}
			</div>
		{/if}

		<!-- Unstaged / untracked files -->
		{#if unstagedFiles.length > 0}
			<div>
				<button
					type="button"
					class="flex w-full items-center gap-1 px-2 py-1 text-xs font-medium text-muted-foreground hover:text-foreground"
					onclick={() => (unstagedExpanded = !unstagedExpanded)}
				>
					{#if unstagedExpanded}
						<ChevronDownIcon class="size-3" />
					{:else}
						<ChevronRightIcon class="size-3" />
					{/if}
					Changes
					<Badge variant="secondary" class="ml-auto h-4 px-1 text-[10px]">
						{unstagedFiles.length}
					</Badge>
				</button>
				{#if unstagedExpanded}
					<div class="space-y-0.5 px-1">
						{#each unstagedFiles as file (file.path + ':unstaged')}
							<FileItem
								{file}
								actionIcon="plus"
								onAction={() => gitStore.stageFiles(path, [file.path])}
								onDiscard={() => gitStore.discardFile(path, file.path)}
							/>
						{/each}
					</div>
				{/if}
			</div>
		{/if}

		<!-- Commit area -->
		{#if stagedFiles.length > 0}
			<div class="flex items-center gap-1 px-2 pt-2">
				<Input
					bind:value={commitMessage}
					placeholder="Commit message..."
					class="h-7 text-xs"
					onkeydown={(e) => {
						if (e.key === 'Enter' && !e.shiftKey) {
							e.preventDefault();
							handleCommit();
						}
					}}
				/>
				<Button
					variant="secondary"
					size="sm"
					class="h-7 shrink-0 gap-1 text-xs"
					disabled={!commitMessage.trim() || committing}
					onclick={handleCommit}
				>
					<CheckIcon class="size-3" />
					Commit
				</Button>
			</div>
		{/if}
	{/if}
</div>
