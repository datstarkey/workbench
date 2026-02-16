<script lang="ts">
	import { onMount } from 'svelte';
	import PlayIcon from '@lucide/svelte/icons/play';
	import SparklesIcon from '@lucide/svelte/icons/sparkles';
	import TerminalSquareIcon from '@lucide/svelte/icons/terminal-square';
	import XIcon from '@lucide/svelte/icons/x';
	import ZapIcon from '@lucide/svelte/icons/zap';
	import { Button } from '$lib/components/ui/button';
	import * as ContextMenu from '$lib/components/ui/context-menu';
	import AgentActionsMenu from '$features/agent-actions/AgentActionsMenu.svelte';
	import { formatSessionDate } from '$lib/utils/format';
	import { effectivePath } from '$lib/utils/path';
	import { getClaudeSessionStore, getProjectStore, getWorkspaceStore } from '$stores/context';
	import type { ProjectWorkspace } from '$types/workbench';

	const workspaceStore = getWorkspaceStore();
	const claudeSessionStore = getClaudeSessionStore();
	const projectStore = getProjectStore();

	let {
		workspace
	}: {
		workspace: ProjectWorkspace;
	} = $props();

	let wsProject = $derived(projectStore.getByPath(workspace.projectPath));
	let wsCwd = $derived(effectivePath(workspace));

	onMount(() => {
		claudeSessionStore.discoverSessions(wsCwd);
		claudeSessionStore.discoverCodexSessions(wsCwd);
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
			<Button
				type="button"
				class="bg-violet-600 hover:bg-violet-700"
				onclick={() => claudeSessionStore.startSessionInWorkspace(workspace)}
			>
				<SparklesIcon class="size-4" />
				New Claude Session
			</Button>
			<Button
				type="button"
				class="bg-sky-600 hover:bg-sky-700"
				onclick={() => claudeSessionStore.startSessionInWorkspace(workspace, 'codex')}
			>
				<ZapIcon class="size-4" />
				New Codex Session
			</Button>
			<Button
				type="button"
				variant="ghost"
				onclick={() => {
					if (wsProject) workspaceStore.addTerminalTab(workspace.id, wsProject);
				}}
			>
				<TerminalSquareIcon class="size-4" />
				New Terminal
			</Button>
			<AgentActionsMenu {workspace} showTextButton />
		</div>

		{#if claudeSessionStore.discoveredSessions.length > 0}
			<div class="mt-8 w-full">
				<h3
					class="mb-2 text-left text-xs font-medium tracking-wider text-muted-foreground uppercase"
				>
					Recent Claude Sessions
				</h3>
				<div class="flex flex-col gap-1">
					{#each claudeSessionStore.discoveredSessions as session (session.sessionId)}
						<ContextMenu.Root>
							<ContextMenu.Trigger class="w-full">
								<button
									type="button"
									class="flex w-full items-center justify-between rounded-md px-3 py-2 text-left text-sm transition-colors hover:bg-muted"
									onclick={() =>
										claudeSessionStore.resumeSession(
											workspace.id,
											session.sessionId,
											session.label
										)}
								>
									<span class="truncate text-foreground">{session.label}</span>
									<span class="ml-3 shrink-0 text-xs text-muted-foreground">
										{formatSessionDate(session.timestamp)}
									</span>
								</button>
							</ContextMenu.Trigger>
							<ContextMenu.Content class="w-40">
								<ContextMenu.Item
									onclick={() =>
										claudeSessionStore.resumeSession(
											workspace.id,
											session.sessionId,
											session.label
										)}
								>
									<PlayIcon class="size-3.5" />
									Resume
								</ContextMenu.Item>
								<ContextMenu.Separator />
								<ContextMenu.Item
									class="text-destructive"
									onclick={() => claudeSessionStore.removeDiscoveredSession(session.sessionId)}
								>
									<XIcon class="size-3.5" />
									Close
								</ContextMenu.Item>
							</ContextMenu.Content>
						</ContextMenu.Root>
					{/each}
				</div>
			</div>
		{/if}

		{#if claudeSessionStore.discoveredCodexSessions.length > 0}
			<div class="mt-8 w-full">
				<h3
					class="mb-2 text-left text-xs font-medium tracking-wider text-muted-foreground uppercase"
				>
					Recent Codex Sessions
				</h3>
				<div class="flex flex-col gap-1">
					{#each claudeSessionStore.discoveredCodexSessions as session (session.sessionId)}
						<ContextMenu.Root>
							<ContextMenu.Trigger class="w-full">
								<button
									type="button"
									class="flex w-full items-center justify-between rounded-md px-3 py-2 text-left text-sm transition-colors hover:bg-muted"
									onclick={() =>
										claudeSessionStore.resumeSession(
											workspace.id,
											session.sessionId,
											session.label,
											'codex'
										)}
								>
									<span class="truncate text-foreground">{session.label}</span>
									<span class="ml-3 shrink-0 text-xs text-muted-foreground">
										{formatSessionDate(session.timestamp)}
									</span>
								</button>
							</ContextMenu.Trigger>
							<ContextMenu.Content class="w-40">
								<ContextMenu.Item
									onclick={() =>
										claudeSessionStore.resumeSession(
											workspace.id,
											session.sessionId,
											session.label,
											'codex'
										)}
								>
									<PlayIcon class="size-3.5" />
									Resume
								</ContextMenu.Item>
								<ContextMenu.Separator />
								<ContextMenu.Item
									class="text-destructive"
									onclick={() =>
										claudeSessionStore.removeDiscoveredSession(session.sessionId, 'codex')}
								>
									<XIcon class="size-3.5" />
									Close
								</ContextMenu.Item>
							</ContextMenu.Content>
						</ContextMenu.Root>
					{/each}
				</div>
			</div>
		{/if}
	</div>
</div>
