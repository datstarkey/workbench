<script lang="ts">
	import TargetIcon from '@lucide/svelte/icons/target';
	import type { GoalInfo } from '@workbench/types';

	let { goal }: { goal: GoalInfo } = $props();
</script>

<!-- Claude keeps taking turns until the condition holds; `/goal clear` ends it early. -->
<div
	class="flex items-start gap-2 rounded-md border border-wb-accent/40 bg-wb-accent/5 px-3 py-1.5 text-xs"
	role="status"
>
	<TargetIcon class="mt-0.5 size-3.5 shrink-0 text-wb-accent" aria-hidden="true" />
	<div class="min-w-0 flex-1">
		<p class="line-clamp-2 text-wb-ink-mute">
			<span class="font-medium text-wb-ink">Goal</span>
			{goal.condition}
		</p>
		{#if goal.reason}
			<p class="line-clamp-2 text-wb-ink-soft" title={goal.reason}>Not met yet: {goal.reason}</p>
		{/if}
	</div>
</div>
