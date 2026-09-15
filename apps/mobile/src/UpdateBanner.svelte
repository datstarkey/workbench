<script lang="ts">
	import { onMount } from 'svelte';
	import type { AppUpdater } from './app-updater.svelte.ts';

	let { updater }: { updater: AppUpdater } = $props();

	const current = $derived(updater.state);
	const percent = $derived(
		current.kind === 'downloading' && current.progress !== null
			? Math.round(current.progress * 100)
			: null
	);

	onMount(() => {
		void updater.checkOnLaunch();
	});
</script>

{#if updater.supported}
	{#if percent !== null}
		<div class="h-0.5 shrink-0 bg-wb-hair">
			<div class="h-full bg-wb-accent transition-[width]" style:width="{percent}%"></div>
		</div>
	{/if}
	<div
		class="flex shrink-0 items-center gap-2 border-t border-wb-hair bg-wb-rail px-3 py-1.5 text-[11px] text-wb-ink-mute"
		style="padding-bottom: calc(0.375rem + env(safe-area-inset-bottom));"
	>
		<span class="min-w-0 flex-1 truncate" role="status">
			{#if current.kind === 'checking'}
				Checking for updates…
			{:else if current.kind === 'up-to-date'}
				Workbench {updater.version} is up to date
			{:else if current.kind === 'available'}
				<span class="text-wb-ink">Update to v{current.update.version}</span>
			{:else if current.kind === 'downloading'}
				Downloading v{current.update.version}{percent === null ? '…' : ` ${percent}%`}
			{:else if current.kind === 'ready'}
				<span class="text-wb-ink">Downloaded v{current.update.version}</span>
			{:else if current.kind === 'installing'}
				Confirm the install in the Android prompt
			{:else if current.kind === 'needs-permission'}
				<span class="text-wb-warn">Allow Workbench to install apps, then retry</span>
			{:else if current.kind === 'error'}
				<span class="text-wb-err">{current.message}</span>
			{:else}
				Workbench {updater.version ?? ''}
			{/if}
		</span>

		{#if current.kind === 'available'}
			<button
				class="rounded bg-wb-accent px-2 py-0.5 font-medium text-wb-bg"
				onclick={() => updater.install()}
			>
				Update
			</button>
			<button class="rounded px-1.5 py-0.5 hover:text-wb-ink" onclick={() => updater.dismiss()}>
				Later
			</button>
		{:else if current.kind === 'ready'}
			<button
				class="rounded bg-wb-accent px-2 py-0.5 font-medium text-wb-bg"
				onclick={() => updater.installDownloaded()}
			>
				Install
			</button>
		{:else if current.kind === 'needs-permission' || current.kind === 'installing' || (current.kind === 'error' && current.update)}
			<button
				class="rounded px-2 py-0.5 text-wb-ink hover:bg-wb-panel2"
				onclick={() => updater.install()}
			>
				Retry
			</button>
		{:else if current.kind === 'idle' || current.kind === 'up-to-date' || current.kind === 'error'}
			<button
				class="rounded px-2 py-0.5 hover:bg-wb-panel2 hover:text-wb-ink"
				onclick={() => updater.check()}
			>
				Check for updates
			</button>
		{/if}
	</div>
{/if}
