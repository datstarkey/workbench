<script lang="ts">
	import CheckIcon from '@lucide/svelte/icons/check';
	import ShieldQuestionIcon from '@lucide/svelte/icons/shield-question-mark';
	import XIcon from '@lucide/svelte/icons/x';
	import { cn } from '@workbench/ui';
	import type { ApprovalDecision } from '$types/workbench';
	import { type ApprovalItem, approvalPreview, shortPath } from './chat-format';

	let {
		approval,
		cwd,
		onDecide
	}: {
		approval: ApprovalItem;
		cwd?: string;
		onDecide: (decision: ApprovalDecision) => void;
	} = $props();

	/** Disables the buttons between a click and the server's confirmation. */
	let sent = $state<ApprovalDecision | null>(null);

	const question = $derived.by(() => {
		switch (approval.tool) {
			case 'Bash':
				return 'Run this command?';
			case 'Edit':
			case 'MultiEdit':
			case 'Write':
			case 'NotebookEdit':
				return 'Change this file?';
			case 'WebFetch':
				return 'Fetch this page?';
			case 'ExitPlanMode':
				return 'Ready to start on this plan?';
			default:
				return `Use ${approval.tool}?`;
		}
	});
	const isPlan = $derived(approval.tool === 'ExitPlanMode');
	const plan = $derived(typeof approval.input?.plan === 'string' ? approval.input.plan : '');
	const preview = $derived(isPlan ? plan : approvalPreview(approval, cwd));
	const isCommand = $derived(approval.tool === 'Bash');
	const outcome = $derived.by(() => {
		if (approval.expired) return 'Withdrawn by Claude';
		switch (approval.decision) {
			case 'allow':
				return isPlan ? 'Plan approved' : 'Allowed';
			case 'alwaysAllow':
				return isPlan ? 'Plan approved, accepting edits' : 'Always allowed';
			case 'deny':
				return isPlan ? 'Kept planning' : 'Denied';
			default:
				return null;
		}
	});

	function decide(decision: ApprovalDecision) {
		sent = decision;
		onDecide(decision);
	}
</script>

{#if outcome}
	<div class="flex min-w-0 items-center gap-2 text-xs text-wb-ink-soft">
		{#if approval.decision === 'deny' || approval.expired}
			<XIcon class="size-3.5 shrink-0" />
		{:else}
			<CheckIcon class="size-3.5 shrink-0 text-wb-ok" />
		{/if}
		<span class="shrink-0">{outcome}</span>
		{#if !isPlan}
			<span class="min-w-0 truncate font-mono text-[11px]">{approval.tool} {preview}</span>
		{/if}
	</div>
{:else}
	<section
		class="approval overflow-hidden rounded-lg border border-wb-warn/45 bg-wb-panel"
		aria-label="Permission request"
	>
		<header class="flex items-center gap-2 px-3.5 pt-3 text-sm">
			<ShieldQuestionIcon class="size-4 shrink-0 text-wb-warn" />
			<span class="font-medium text-wb-ink">{question}</span>
			{#if !isPlan}<span class="ml-auto text-xs text-wb-ink-soft">{approval.tool}</span>{/if}
		</header>
		{#if isPlan}
			<div
				class="scrollbar-thin mx-3.5 mt-2.5 max-h-80 overflow-auto rounded-md border border-wb-hair bg-wb-bg px-3.5 py-3 text-sm leading-relaxed whitespace-pre-wrap text-wb-ink"
			>
				{preview}
			</div>
		{:else}
			<pre
				class={cn(
					'scrollbar-thin mx-3.5 mt-2.5 max-h-48 overflow-auto rounded-md border border-wb-hair bg-wb-bg px-3 py-2 font-mono text-xs leading-relaxed whitespace-pre-wrap text-wb-ink',
					isCommand && 'command'
				)}>{preview}</pre>
		{/if}
		{#if approval.description || approval.blockedPath}
			<div class="flex flex-col gap-0.5 px-3.5 pt-2 text-xs text-wb-ink-mute">
				{#if approval.description}<p>{approval.description}</p>{/if}
				{#if approval.blockedPath}
					<p>
						Needs access to <span class="font-mono">{shortPath(approval.blockedPath, cwd)}</span>
					</p>
				{/if}
			</div>
		{/if}
		<div class="flex flex-wrap items-center gap-2 px-3.5 py-3">
			<button
				type="button"
				class="rounded-md bg-wb-accent px-3 py-1 text-xs font-semibold text-wb-accent-ink hover:brightness-110 focus-visible:ring-2 focus-visible:ring-wb-accent/50 focus-visible:outline-none disabled:opacity-50"
				disabled={sent !== null}
				onclick={() => decide('allow')}
			>
				{sent === 'allow' ? 'Sending…' : isPlan ? 'Start building' : 'Allow'}
			</button>
			{#if approval.canAlwaysAllow}
				<button
					type="button"
					class="rounded-md border border-wb-hair bg-wb-panel2 px-3 py-1 text-xs text-wb-ink hover:border-wb-ink-soft focus-visible:ring-2 focus-visible:ring-wb-accent/50 focus-visible:outline-none disabled:opacity-50"
					disabled={sent !== null}
					onclick={() => decide('alwaysAllow')}
				>
					{isPlan ? 'Start, accept edits' : 'Always allow'}
				</button>
			{/if}
			<button
				type="button"
				class="ml-auto rounded-md px-3 py-1 text-xs text-wb-ink-mute hover:text-wb-err focus-visible:ring-2 focus-visible:ring-wb-accent/50 focus-visible:outline-none disabled:opacity-50"
				disabled={sent !== null}
				onclick={() => decide('deny')}
			>
				{isPlan ? 'Keep planning' : 'Deny'}
			</button>
		</div>
	</section>
{/if}

<style>
	/* Arrives with a short lift so a request that lands mid-scroll is noticed. */
	.approval {
		animation: arrive 260ms cubic-bezier(0.2, 0.8, 0.2, 1);
		box-shadow: 0 0 0 3px color-mix(in oklab, var(--wb-warn) 10%, transparent);
	}
	@keyframes arrive {
		from {
			opacity: 0;
			transform: translateY(6px);
		}
	}
	.command::before {
		content: '$ ';
		color: var(--wb-ink-soft);
	}
	@media (prefers-reduced-motion: reduce) {
		.approval {
			animation: none;
		}
	}
</style>
