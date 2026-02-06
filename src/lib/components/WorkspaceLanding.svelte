<script lang="ts">
	import { onMount } from 'svelte';
	import SparklesIcon from '@lucide/svelte/icons/sparkles';
	import TerminalSquareIcon from '@lucide/svelte/icons/terminal-square';
	import { Button } from '$lib/components/ui/button';
	import type { DiscoveredClaudeSession } from '$types/workbench';

	let {
		sessions,
		onNewClaude,
		onResume,
		onDiscover,
		onNewTerminal
	}: {
		sessions: DiscoveredClaudeSession[];
		onNewClaude: () => void;
		onResume: (sessionId: string, label: string) => void;
		onDiscover: () => void;
		onNewTerminal: () => void;
	} = $props();

	function formatDate(timestamp: string): string {
		const date = new Date(timestamp);
		return date.toLocaleString('en-US', {
			month: 'short',
			day: 'numeric',
			hour: 'numeric',
			minute: '2-digit',
			hour12: true
		});
	}

	onMount(() => {
		onDiscover();
	});
</script>

<div class="flex flex-1 items-center justify-center">
	<div class="flex w-full max-w-md flex-col items-center text-center">
		<div
			class="mb-4 flex size-12 items-center justify-center rounded-xl bg-violet-500/10 text-violet-400"
		>
			<SparklesIcon class="size-6" />
		</div>

		<h2 class="text-lg font-semibold tracking-tight">Start a session</h2>

		<div class="mt-4 flex gap-2">
			<Button type="button" class="bg-violet-600 hover:bg-violet-700" onclick={onNewClaude}>
				<SparklesIcon class="size-4" />
				New Claude Session
			</Button>
			<Button type="button" variant="ghost" onclick={onNewTerminal}>
				<TerminalSquareIcon class="size-4" />
				New Terminal
			</Button>
		</div>

		{#if sessions.length > 0}
			<div class="mt-8 w-full">
				<h3
					class="mb-2 text-left text-xs font-medium tracking-wider text-muted-foreground uppercase"
				>
					Recent Sessions
				</h3>
				<div class="flex flex-col gap-1">
					{#each sessions as session (session.sessionId)}
						<button
							type="button"
							class="flex w-full items-center justify-between rounded-md px-3 py-2 text-left text-sm transition-colors hover:bg-muted"
							onclick={() => onResume(session.sessionId, session.label)}
						>
							<span class="truncate text-foreground">{session.label}</span>
							<span class="ml-3 shrink-0 text-xs text-muted-foreground">
								{formatDate(session.timestamp)}
							</span>
						</button>
					{/each}
				</div>
			</div>
		{/if}
	</div>
</div>
