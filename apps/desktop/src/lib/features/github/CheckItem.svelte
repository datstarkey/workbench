<script lang="ts">
	import CircleCheckIcon from '@lucide/svelte/icons/circle-check';
	import CircleMinusIcon from '@lucide/svelte/icons/circle-minus';
	import CircleXIcon from '@lucide/svelte/icons/circle-x';
	import ExternalLinkIcon from '@lucide/svelte/icons/external-link';
	import LoaderCircleIcon from '@lucide/svelte/icons/loader-circle';
	import RotateCcwIcon from '@lucide/svelte/icons/rotate-ccw';
	import { cn } from '@workbench/ui';
	import { iconButton, outlineButton } from '$features/sidebar/styles';
	import { openInGitHub } from '$lib/utils/github';
	import { formatDuration, type RunCheck } from './pr-view';

	let { check, onRerun }: { check: RunCheck; onRerun?: () => Promise<void> } = $props();

	let rerunning = $state(false);
	let duration = $derived(formatDuration(check.startedAt, check.completedAt));
	let failed = $derived(check.bucket === 'fail');

	async function rerun() {
		if (!onRerun) return;
		rerunning = true;
		try {
			await onRerun();
		} finally {
			rerunning = false;
		}
	}
</script>

{#snippet summary()}
	{#if check.bucket === 'pass'}
		<CircleCheckIcon class="size-3.5 shrink-0 text-wb-ok" />
	{:else if check.bucket === 'fail'}
		<CircleXIcon class="size-3.5 shrink-0 text-wb-err" />
	{:else if check.bucket === 'pending'}
		<LoaderCircleIcon class="size-3.5 shrink-0 animate-spin text-wb-warn" />
	{:else}
		<CircleMinusIcon class="size-3.5 shrink-0 text-wb-ink-soft" />
	{/if}
	<div class="flex min-w-0 flex-1 flex-col gap-px">
		<span class="truncate text-xs text-wb-ink" title={check.name}>{check.name}</span>
		{#if check.subtitle && check.subtitle !== check.name}
			<span class="truncate text-[10.5px] text-wb-ink-soft" title={check.subtitle}>
				{check.subtitle}
			</span>
		{/if}
	</div>
	{#if duration}
		<span class="shrink-0 font-mono text-[10.5px] text-wb-ink-soft">{duration}</span>
	{/if}
{/snippet}

{#if failed}
	<div
		class="mx-2 mb-1.5 flex flex-col gap-1.5 rounded-md border border-wb-err/35 bg-wb-err/[0.07] px-2.5 py-2"
	>
		<div class="flex items-center gap-2">{@render summary()}</div>
		{#if check.description}
			<p class="pl-[22px] text-[11px] leading-[1.4] text-wb-ink-mute">{check.description}</p>
		{/if}
		<div class="flex gap-1.5 pl-[22px]">
			{#if onRerun}
				<button type="button" class={outlineButton} disabled={rerunning} onclick={rerun}>
					<RotateCcwIcon class={['size-[11px]', rerunning && 'animate-spin']} />
					Re-run
				</button>
			{/if}
			{#if check.link}
				<button
					type="button"
					class={cn(outlineButton, 'bg-transparent')}
					onclick={() => openInGitHub(check.link)}
				>
					<ExternalLinkIcon class="size-[11px]" />
					View logs
				</button>
			{/if}
		</div>
	</div>
{:else}
	<div
		class="group mx-2 flex min-h-7 items-center gap-2 rounded-[5px] px-2.5 py-1 hover:bg-wb-panel2"
	>
		{@render summary()}
		{#if check.link}
			<button
				type="button"
				class={[iconButton, 'opacity-0 group-focus-within:opacity-100 group-hover:opacity-100']}
				aria-label="Open {check.name} on GitHub"
				onclick={() => openInGitHub(check.link)}
			>
				<ExternalLinkIcon class="size-3" />
			</button>
		{/if}
	</div>
{/if}
