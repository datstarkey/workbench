<script lang="ts">
	import ArrowDownIcon from '@lucide/svelte/icons/arrow-down';
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import CircleCheckIcon from '@lucide/svelte/icons/circle-check';
	import CircleMinusIcon from '@lucide/svelte/icons/circle-minus';
	import CircleXIcon from '@lucide/svelte/icons/circle-x';
	import EyeIcon from '@lucide/svelte/icons/eye';
	import GitMergeIcon from '@lucide/svelte/icons/git-merge';
	import LoaderCircleIcon from '@lucide/svelte/icons/loader-circle';
	import LoaderIcon from '@lucide/svelte/icons/loader';
	import SendHorizontalIcon from '@lucide/svelte/icons/send-horizontal';
	import { cn } from '@workbench/ui';
	import * as DropdownMenu from '@workbench/ui/dropdown-menu';
	import { invoke } from '@tauri-apps/api/core';
	import { outlineButton, primaryButton } from '$features/sidebar/styles';
	import { getGitHubStore } from '$stores/context';
	import type { GitHubPR, MergePrOptions } from '$types/workbench';
	import { toast } from 'svelte-sonner';
	import { mergeMode, readinessRows, TONE_TEXT, type ReadinessRow } from './pr-view';

	let { pr, projectPath }: { pr: GitHubPR; projectPath: string } = $props();

	const githubStore = getGitHubStore();

	const METHOD_LABELS: Record<MergePrOptions['method'], string> = {
		squash: 'Squash and merge',
		merge: 'Create a merge commit',
		rebase: 'Rebase and merge'
	};

	let method = $state<MergePrOptions['method']>('squash');
	let deleteBranch = $state(true);
	let admin = $state(false);
	let busy = $state<'merge' | 'auto' | 'update' | 'ready' | null>(null);

	let rows = $derived(readinessRows(pr));
	let mode = $derived(mergeMode(pr));

	function rowIcon(row: ReadinessRow) {
		if (row.tone === 'ok') return CircleCheckIcon;
		if (row.tone === 'err') return CircleXIcon;
		if (row.tone === 'mute') return CircleMinusIcon;
		if (row.key === 'checks') return LoaderCircleIcon;
		return row.key === 'review' ? EyeIcon : ArrowDownIcon;
	}

	async function run(kind: NonNullable<typeof busy>, command: string, success: string, extra = {}) {
		busy = kind;
		try {
			await invoke(command, { projectPath, prNumber: pr.number, ...extra });
			toast.success(success);
			await githubStore.refreshProject(projectPath);
		} catch (e) {
			toast.error(String(e));
		} finally {
			busy = null;
		}
	}

	function merge(auto: boolean) {
		// gh rejects --admin together with --auto
		const options: MergePrOptions = { method, deleteBranch, admin: admin && !auto, auto };
		return run(
			auto ? 'auto' : 'merge',
			'github_merge_pr',
			auto ? `Auto-merge enabled for #${pr.number}` : `Merged #${pr.number}`,
			{ options }
		);
	}
</script>

{#snippet mergeMenu(triggerClass: string)}
	<DropdownMenu.Root>
		<DropdownMenu.Trigger class={triggerClass} aria-label="Merge options" disabled={busy !== null}>
			<ChevronDownIcon class="size-3.5" />
		</DropdownMenu.Trigger>
		<DropdownMenu.Content align="end" class="w-56">
			<DropdownMenu.Label class="text-xs text-muted-foreground">Method</DropdownMenu.Label>
			<DropdownMenu.RadioGroup bind:value={method}>
				{#each Object.entries(METHOD_LABELS) as [value, label] (value)}
					<DropdownMenu.RadioItem {value}>{label}</DropdownMenu.RadioItem>
				{/each}
			</DropdownMenu.RadioGroup>
			<DropdownMenu.Separator />
			<DropdownMenu.CheckboxItem bind:checked={deleteBranch}>
				Delete branch after merge
			</DropdownMenu.CheckboxItem>
			<DropdownMenu.CheckboxItem bind:checked={admin}
				>Bypass rules (admin)</DropdownMenu.CheckboxItem
			>
			<DropdownMenu.Separator />
			{#if mode === 'merge'}
				<DropdownMenu.Item onclick={() => merge(true)}>Enable auto-merge instead</DropdownMenu.Item>
			{:else if pr.actions.canMerge || admin}
				<!-- Failing/pending checks only merge now when the user opts into bypassing rules -->
				<DropdownMenu.Item onclick={() => merge(false)}>
					{admin ? 'Merge now, bypassing rules' : 'Merge now'}
				</DropdownMenu.Item>
			{/if}
		</DropdownMenu.Content>
	</DropdownMenu.Root>
{/snippet}

<div
	class={[
		'flex flex-col overflow-hidden rounded-lg border bg-wb-bg',
		mode === 'merge' ? 'border-wb-ok/35' : 'border-wb-hair'
	]}
>
	{#each rows as row (row.key)}
		{@const Icon = rowIcon(row)}
		<div class="flex min-h-9 items-center gap-2 border-b border-wb-hair-soft px-2.5 py-1.5">
			<Icon
				class={[
					'size-[15px] shrink-0',
					TONE_TEXT[row.tone],
					row.key === 'checks' && row.tone === 'warn' && 'animate-spin'
				]}
			/>
			<div class="flex min-w-0 flex-1 flex-col">
				<span class="text-xs font-medium text-wb-ink">{row.label}</span>
				{#if row.detail}
					<span class="text-[10.5px] text-wb-ink-soft">{row.detail}</span>
				{/if}
			</div>
			{#if row.key === 'branch' && pr.actions.canUpdateBranch}
				<button
					type="button"
					class={outlineButton}
					disabled={busy !== null}
					onclick={() => run('update', 'github_update_pr_branch', 'Branch updated from base')}
				>
					{#if busy === 'update'}<LoaderIcon class="size-3 animate-spin" />{/if}
					Update
				</button>
			{/if}
		</div>
	{/each}

	{#if mode}
		<div class="flex flex-col gap-1.5 p-2.5">
			{#if mode === 'ready-for-review'}
				<button
					type="button"
					class={cn(primaryButton, 'rounded-md')}
					disabled={busy !== null}
					onclick={() => run('ready', 'github_mark_pr_ready', `#${pr.number} is ready for review`)}
				>
					{#if busy === 'ready'}
						<LoaderIcon class="size-3.5 animate-spin" />
					{:else}
						<SendHorizontalIcon class="size-3.5" />
					{/if}
					Ready for review
				</button>
			{:else if mode === 'merge'}
				<div class="flex">
					<button
						type="button"
						class={cn(primaryButton, 'flex-1 rounded-l-md bg-wb-ok hover:bg-wb-ok/90')}
						disabled={busy !== null}
						onclick={() => merge(false)}
					>
						{#if busy === 'merge'}
							<LoaderIcon class="size-3.5 animate-spin" />
						{:else}
							<GitMergeIcon class="size-3.5" />
						{/if}
						{METHOD_LABELS[method]}
					</button>
					{@render mergeMenu(
						cn(
							primaryButton,
							'w-7 rounded-r-md border-l border-wb-accent-ink/25 bg-wb-ok px-0 hover:bg-wb-ok/90'
						)
					)}
				</div>
			{:else}
				<div class="flex">
					<button
						type="button"
						class={cn(
							primaryButton,
							'flex-1 rounded-l-md border border-wb-hair bg-wb-panel2 text-wb-ink hover:bg-wb-hair'
						)}
						disabled={busy !== null}
						onclick={() => merge(true)}
					>
						{#if busy === 'auto'}
							<LoaderIcon class="size-3.5 animate-spin" />
						{:else}
							<GitMergeIcon class="size-3.5" />
						{/if}
						Auto-merge when ready
					</button>
					{@render mergeMenu(
						cn(
							primaryButton,
							'w-7 rounded-r-md border border-l-0 border-wb-hair bg-wb-panel2 px-0 text-wb-ink-mute hover:bg-wb-hair'
						)
					)}
				</div>
				<span class="text-center text-[10.5px] text-wb-ink-soft">
					Merges automatically once every requirement passes
				</span>
			{/if}
		</div>
	{/if}
</div>
