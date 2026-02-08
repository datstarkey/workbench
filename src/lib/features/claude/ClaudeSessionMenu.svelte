<script lang="ts">
	import HistoryIcon from '@lucide/svelte/icons/history';
	import { Button } from '$lib/components/ui/button';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import * as Tooltip from '$lib/components/ui/tooltip';
	import { getClaudeSessionStore } from '$stores/context';
	import { formatSessionDate } from '$lib/utils/format';

	const claudeSessionStore = getClaudeSessionStore();

	let {
		onResume,
		onOpen
	}: {
		onResume: (sessionId: string, label: string) => void;
		onOpen: () => void;
	} = $props();
</script>

<DropdownMenu.Root
	onOpenChange={(open) => {
		if (open) onOpen();
	}}
>
	<Tooltip.Root>
		<Tooltip.Trigger>
			<DropdownMenu.Trigger>
				{#snippet child({ props })}
					<Button
						{...props}
						variant="ghost"
						size="icon-sm"
						class="size-7 text-muted-foreground hover:text-foreground"
						type="button"
					>
						<HistoryIcon class="size-3.5" />
					</Button>
				{/snippet}
			</DropdownMenu.Trigger>
		</Tooltip.Trigger>
		<Tooltip.Content>Resume Claude Session</Tooltip.Content>
	</Tooltip.Root>
	<DropdownMenu.Content align="end" class="max-h-80 w-72 overflow-y-auto">
		<DropdownMenu.Label>Past Sessions</DropdownMenu.Label>
		<DropdownMenu.Separator />
		{#if claudeSessionStore.discoveredSessions.length === 0}
			<div class="px-2 py-3 text-center text-xs text-muted-foreground">No past sessions found</div>
		{:else}
			{#each claudeSessionStore.discoveredSessions as session (session.sessionId)}
				<DropdownMenu.Item onclick={() => onResume(session.sessionId, session.label)}>
					<div class="flex flex-col gap-0.5">
						<span class="line-clamp-1 text-xs font-medium">{session.label}</span>
						<span class="text-[10px] text-muted-foreground"
							>{formatSessionDate(session.timestamp)}</span
						>
					</div>
				</DropdownMenu.Item>
			{/each}
		{/if}
	</DropdownMenu.Content>
</DropdownMenu.Root>
