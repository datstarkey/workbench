<script lang="ts">
	import TargetIcon from '@lucide/svelte/icons/target';
	import type { GoalInfo } from '@workbench/types';

	let { goal }: { goal: GoalInfo } = $props();

	const notMet = $derived(goal.reason ? `Not met yet: ${goal.reason}` : undefined);
</script>

<!-- Claude keeps taking turns until the condition holds; `/goal clear` ends it early. -->
<div
	class="flex items-start gap-2 rounded-md border border-wb-accent/40 bg-wb-accent/5 px-3 py-1.5 text-xs"
	role="status"
	title={notMet}
>
	<TargetIcon class="mt-0.5 size-3.5 shrink-0 text-wb-accent" aria-hidden="true" />
	<p class="line-clamp-2 min-w-0 flex-1 text-wb-ink-mute">
		<span class="font-medium text-wb-ink">Goal</span>
		{goal.condition}
	</p>
	{#if notMet}
		<span class="sr-only">{notMet}</span>
	{/if}
</div>
