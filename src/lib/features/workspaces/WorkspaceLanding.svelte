<script lang="ts">
	import { onMount } from 'svelte';
	import SparklesIcon from '@lucide/svelte/icons/sparkles';
	import TerminalSquareIcon from '@lucide/svelte/icons/terminal-square';
	import ZapIcon from '@lucide/svelte/icons/zap';
	import { Button } from '$lib/components/ui/button';
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
