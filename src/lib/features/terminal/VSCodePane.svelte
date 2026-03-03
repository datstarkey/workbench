<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import LoaderCircleIcon from '@lucide/svelte/icons/loader-circle';
	import {
		startCodeServer,
		stopCodeServer,
		waitForReady,
		onCodeServerExit
	} from '$lib/utils/code-server';
	import type { CodeServerInfo } from '$types/workbench';

	let {
		sessionId,
		projectPath
	}: {
		sessionId: string;
		projectPath: string;
	} = $props();

	let serverInfo: CodeServerInfo | null = $state(null);
	let loading = $state(true);
	let error = $state('');
	let unlistenExit: (() => void) | null = null;

	onMount(async () => {
		try {
			const info = await startCodeServer(sessionId, projectPath);
			const ready = await waitForReady(info.url);
			if (!ready) {
				error = 'code-server started but did not become ready within 15 seconds.';
				loading = false;
				return;
			}
			serverInfo = info;
			loading = false;

			unlistenExit = await onCodeServerExit(sessionId, (event) => {
				error = `code-server exited unexpectedly (code: ${event.exitCode})`;
				serverInfo = null;
			});
		} catch (e) {
			error = String(e);
			loading = false;
		}
	});

	onDestroy(() => {
		unlistenExit?.();
		void stopCodeServer(sessionId);
	});
</script>

<div class="h-full w-full overflow-hidden">
	{#if loading}
		<div class="flex h-full items-center justify-center text-muted-foreground">
			<LoaderCircleIcon class="mr-2 size-4 animate-spin" />
			<span class="text-sm">Starting VS Code...</span>
		</div>
	{:else if error}
		<div class="flex h-full items-center justify-center">
			<div class="max-w-sm text-center">
				<p class="text-sm text-red-400">{error}</p>
				{#if error.includes('not found') || error.includes('not installed') || error.includes('Install')}
					<p class="mt-3 text-sm text-muted-foreground">
						Install with:
						<code class="rounded bg-muted px-1.5 py-0.5 text-xs">brew install code-server</code>
					</p>
					<p class="mt-1 text-xs text-muted-foreground">
						or visit
						<a
							href="https://coder.com/docs/code-server/install"
							target="_blank"
							class="underline hover:text-foreground">coder.com</a
						>
					</p>
				{/if}
			</div>
		</div>
	{:else if serverInfo}
		<iframe
			src={serverInfo.url}
			title="VS Code"
			class="h-full w-full border-0"
			sandbox="allow-scripts allow-same-origin allow-forms allow-popups allow-modals"
		></iframe>
	{/if}
</div>
