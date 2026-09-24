<script lang="ts">
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import CircleCheckIcon from '@lucide/svelte/icons/circle-check';
	import SidebarSection from '$features/sidebar/SidebarSection.svelte';
	import { textButton } from '$features/sidebar/styles';
	import { getGitHubStore } from '$stores/context';
	import { toast } from 'svelte-sonner';
	import CheckItem from './CheckItem.svelte';
	import { groupChecks, type RunCheck } from './pr-view';

	let {
		title,
		checks,
		projectPath,
		emptyText
	}: { title: string; checks: RunCheck[]; projectPath: string; emptyText: string } = $props();

	const githubStore = getGitHubStore();

	let showSettled = $state(false);
	let groups = $derived(groupChecks(checks));
	let settled = $derived([...groups.passed, ...groups.skipped]);
	let failedRunIds = $derived(
		groups.failing
			.map((c) => c.runId)
			.filter((id, i, ids): id is number => id !== null && ids.indexOf(id) === i)
	);

	const checkKey = (c: RunCheck) => `${c.runId ?? ''}:${c.workflow}:${c.name}`;
	let settledLabel = $derived(
		[
			groups.passed.length ? `${groups.passed.length} passed` : '',
			groups.skipped.length ? `${groups.skipped.length} skipped` : ''
		]
			.filter(Boolean)
			.join(' · ')
	);

	async function rerun(runIds: number[]) {
		try {
			await githubStore.rerunWorkflows(projectPath, runIds);
			toast.success(runIds.length === 1 ? 'Re-run started' : `${runIds.length} re-runs started`);
		} catch (e) {
			toast.error(`Failed to re-run: ${e}`);
		}
	}
</script>

<SidebarSection {title} collapsible={false}>
	{#snippet actions()}
		{#if failedRunIds.length > 1}
			<button type="button" class={textButton} onclick={() => rerun(failedRunIds)}>
				Re-run failed
			</button>
		{/if}
	{/snippet}
	{#if checks.length === 0}
		<p class="px-3 py-1 text-xs text-wb-ink-soft">{emptyText}</p>
	{/if}
	{#each groups.failing as check (checkKey(check))}
		<CheckItem {check} onRerun={check.runId === null ? undefined : () => rerun([check.runId!])} />
	{/each}
	{#each groups.pending as check (checkKey(check))}
		<CheckItem {check} />
	{/each}
	{#if settled.length > 0}
		<button
			type="button"
			class="mx-2 flex h-7 items-center gap-2 rounded-[5px] px-2.5 text-left text-xs text-wb-ink-mute hover:bg-wb-panel2 hover:text-wb-ink"
			aria-expanded={showSettled}
			onclick={() => (showSettled = !showSettled)}
		>
			<CircleCheckIcon class="size-3.5 shrink-0 text-wb-ok" />
			<span class="flex-1">{settledLabel}</span>
			{#if showSettled}
				<ChevronDownIcon class="size-3" />
			{:else}
				<ChevronRightIcon class="size-3" />
			{/if}
		</button>
		{#if showSettled}
			{#each settled as check (checkKey(check))}
				<CheckItem {check} />
			{/each}
		{/if}
	{/if}
</SidebarSection>
