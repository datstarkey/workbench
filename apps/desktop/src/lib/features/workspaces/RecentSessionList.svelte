<script lang="ts">
	import { AgentIcon } from '@workbench/ui/agent-icon';
	import PlayIcon from '@lucide/svelte/icons/play';
	import XIcon from '@lucide/svelte/icons/x';
	import * as ContextMenu from '@workbench/ui/context-menu';
	import { formatSessionDate } from '$lib/utils/format';
	import type { DiscoveredClaudeSession } from '$types/workbench';

	let {
		agent,
		title,
		sessions,
		onResume,
		onRemove
	}: {
		agent: 'claude' | 'codex';
		title: string;
		sessions: DiscoveredClaudeSession[];
		onResume: (sessionId: string, label: string) => void;
		onRemove: (sessionId: string) => void;
	} = $props();
</script>

{#if sessions.length > 0}
	<div class="mt-8 w-full">
		<h3
			class="mb-2 flex items-center gap-2 text-left text-xs font-medium tracking-wider text-muted-foreground uppercase"
		>
			<AgentIcon {agent} class="size-3.5" />{title}
		</h3>
		<div class="flex flex-col gap-1">
			{#each sessions as session (session.sessionId)}
				<ContextMenu.Root>
					<ContextMenu.Trigger class="w-full">
						<button
							type="button"
							class="flex w-full items-center justify-between rounded-md px-3 py-2 text-left text-sm transition-colors hover:bg-muted"
							onclick={() => onResume(session.sessionId, session.label)}
						>
							<span class="truncate text-foreground">{session.label}</span>
							<span class="ml-3 shrink-0 text-xs text-muted-foreground">
								{formatSessionDate(session.timestamp)}
							</span>
						</button>
					</ContextMenu.Trigger>
					<ContextMenu.Content class="w-40">
						<ContextMenu.Item onclick={() => onResume(session.sessionId, session.label)}>
							<PlayIcon class="size-3.5" />
							Resume
						</ContextMenu.Item>
						<ContextMenu.Separator />
						<ContextMenu.Item class="text-destructive" onclick={() => onRemove(session.sessionId)}>
							<XIcon class="size-3.5" />
							Close
						</ContextMenu.Item>
					</ContextMenu.Content>
				</ContextMenu.Root>
			{/each}
		</div>
	</div>
{/if}
