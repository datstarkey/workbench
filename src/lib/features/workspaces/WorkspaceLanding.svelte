<script lang="ts">
	import { onMount } from 'svelte';
	import SparklesIcon from '@lucide/svelte/icons/sparkles';
	import TerminalSquareIcon from '@lucide/svelte/icons/terminal-square';
	import ZapIcon from '@lucide/svelte/icons/zap';
	import AgentActionsMenu from '$features/agent-actions/AgentActionsMenu.svelte';
	import RecentSessionList from '$features/workspaces/RecentSessionList.svelte';
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

<div class="flex flex-1 items-center justify-center bg-wb-bg">
	<div class="flex w-full max-w-[540px] flex-col items-center gap-6 px-4 text-center">
		<!-- Hero icon + headline -->
		<div class="flex flex-col items-center gap-3">
			<div class="grid size-10 place-items-center rounded-xl bg-wb-accent/10 text-wb-accent">
				<SparklesIcon class="size-5" />
			</div>
			<div>
				<h2 class="text-[15px] font-semibold tracking-tight text-wb-ink">Start a session</h2>
				<p class="mt-0.5 text-[12px] text-wb-ink-soft">
					Pick an AI agent or open a plain terminal to get started.
				</p>
			</div>
		</div>

		<!-- Action rows -->
		<div class="flex w-full flex-col gap-2">
			<!-- Claude — primary action -->
			<button
				type="button"
				onclick={() => claudeSessionStore.startSessionInWorkspace(workspace)}
				class="group flex w-full items-center gap-3 rounded-lg border border-wb-accent bg-wb-panel px-3 py-2.5 ring-2 ring-wb-accent/20 transition-colors hover:bg-wb-panel2"
			>
				<div
					class="grid size-8 shrink-0 place-items-center rounded-md bg-wb-bg text-wb-accent transition-colors group-hover:bg-wb-accent/10"
				>
					<SparklesIcon class="size-4" />
				</div>
				<div class="flex flex-1 flex-col items-start text-left">
					<span class="text-[13px] font-medium text-wb-ink">New Claude Session</span>
					<span class="text-[11.5px] text-wb-ink-soft">AI coding assistant with full context</span>
				</div>
				<span class="font-mono text-[11px] text-wb-ink-soft">claude</span>
			</button>

			<!-- Codex -->
			<button
				type="button"
				onclick={() => claudeSessionStore.startSessionInWorkspace(workspace, 'codex')}
				class="group flex w-full items-center gap-3 rounded-lg border border-wb-hair bg-wb-panel px-3 py-2.5 transition-colors hover:bg-wb-panel2"
			>
				<div
					class="grid size-8 shrink-0 place-items-center rounded-md bg-wb-bg text-wb-codex transition-colors group-hover:bg-wb-codex/10"
				>
					<ZapIcon class="size-4" />
				</div>
				<div class="flex flex-1 flex-col items-start text-left">
					<span class="text-[13px] font-medium text-wb-ink">New Codex Session</span>
					<span class="text-[11.5px] text-wb-ink-soft">OpenAI Codex agent in your terminal</span>
				</div>
				<span class="font-mono text-[11px] text-wb-ink-soft">codex</span>
			</button>

			<!-- Shell -->
			<button
				type="button"
				onclick={() => {
					if (wsProject) workspaceStore.addTerminalTab(workspace.id, wsProject);
				}}
				class="group flex w-full items-center gap-3 rounded-lg border border-wb-hair bg-wb-panel px-3 py-2.5 transition-colors hover:bg-wb-panel2"
			>
				<div
					class="grid size-8 shrink-0 place-items-center rounded-md bg-wb-bg text-wb-shell transition-colors group-hover:bg-wb-shell/10"
				>
					<TerminalSquareIcon class="size-4" />
				</div>
				<div class="flex flex-1 flex-col items-start text-left">
					<span class="text-[13px] font-medium text-wb-ink">New Terminal</span>
					<span class="text-[11.5px] text-wb-ink-soft">Plain interactive shell session</span>
				</div>
				<span class="font-mono text-[11px] text-wb-ink-soft">$</span>
			</button>

			<!-- Agent actions (keeps its own button/menu) -->
			<div class="flex justify-start">
				<AgentActionsMenu {workspace} showTextButton />
			</div>
		</div>

		<!-- Recent sessions -->
		<div class="w-full">
			<RecentSessionList
				title="Recent Claude Sessions"
				sessions={claudeSessionStore.discoveredSessions}
				onResume={(id, label) => claudeSessionStore.resumeSession(workspace.id, id, label)}
				onRemove={(id) => claudeSessionStore.removeDiscoveredSession(id)}
			/>
			<RecentSessionList
				title="Recent Codex Sessions"
				sessions={claudeSessionStore.discoveredCodexSessions}
				onResume={(id, label) => claudeSessionStore.resumeSession(workspace.id, id, label, 'codex')}
				onRemove={(id) => claudeSessionStore.removeDiscoveredSession(id, 'codex')}
			/>
		</div>
	</div>
</div>
