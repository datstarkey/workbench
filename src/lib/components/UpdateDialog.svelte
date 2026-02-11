<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import * as Dialog from '$lib/components/ui/dialog';
	import { getUpdaterStore } from '$stores/context';
	import LoaderIcon from '@lucide/svelte/icons/loader';
	import CheckCircleIcon from '@lucide/svelte/icons/circle-check';
	import AlertCircleIcon from '@lucide/svelte/icons/circle-alert';
	import DownloadIcon from '@lucide/svelte/icons/download';

	const updaterStore = getUpdaterStore();

	let isDownloading = $derived(updaterStore.status === 'downloading');

	let progressPercent = $derived(
		updaterStore.contentLength > 0
			? Math.round((updaterStore.progress / updaterStore.contentLength) * 100)
			: null
	);

	function handleOpenChange(open: boolean) {
		if (!open && !isDownloading) {
			updaterStore.dismiss();
		}
	}
</script>

<Dialog.Root open={updaterStore.dialogOpen} onOpenChange={handleOpenChange}>
	<Dialog.Content
		class="sm:max-w-sm"
		showCloseButton={!isDownloading}
		onInteractOutside={(e) => {
			if (isDownloading) e.preventDefault();
		}}
		onEscapeKeydown={(e) => {
			if (isDownloading) e.preventDefault();
		}}
	>
		{#if updaterStore.status === 'checking'}
			<div class="flex flex-col items-center gap-3 py-6">
				<LoaderIcon class="size-8 animate-spin text-muted-foreground" />
				<p class="text-sm text-muted-foreground">Checking for updates...</p>
			</div>
		{:else if updaterStore.status === 'available'}
			<Dialog.Header>
				<Dialog.Title>Update Available</Dialog.Title>
				<Dialog.Description>Version {updaterStore.version} is ready to install.</Dialog.Description>
			</Dialog.Header>

			{#if updaterStore.body}
				<div class="max-h-48 overflow-y-auto rounded-md border p-3">
					<p class="whitespace-pre-wrap text-sm text-muted-foreground">{updaterStore.body}</p>
				</div>
			{/if}

			<Dialog.Footer>
				<Button variant="ghost" onclick={() => updaterStore.dismiss()}>Later</Button>
				<Button onclick={() => updaterStore.downloadAndInstall()}>
					<DownloadIcon class="size-4" />
					Update Now
				</Button>
			</Dialog.Footer>
		{:else if updaterStore.status === 'downloading'}
			<Dialog.Header>
				<Dialog.Title>Downloading Update</Dialog.Title>
				<Dialog.Description>
					{#if progressPercent !== null}
						{progressPercent}% complete
					{:else}
						Downloading...
					{/if}
				</Dialog.Description>
			</Dialog.Header>

			<div class="py-2">
				<div class="h-2 w-full overflow-hidden rounded-full bg-secondary">
					{#if progressPercent !== null}
						<div
							class="h-full rounded-full bg-primary transition-all duration-300"
							style="width: {progressPercent}%"
						></div>
					{:else}
						<div class="h-full w-1/3 animate-pulse rounded-full bg-primary"></div>
					{/if}
				</div>
			</div>
		{:else if updaterStore.status === 'up-to-date'}
			<div class="flex flex-col items-center gap-3 py-6">
				<CheckCircleIcon class="size-8 text-green-500" />
				<p class="text-sm text-muted-foreground">You're up to date.</p>
			</div>

			<Dialog.Footer>
				<Button variant="ghost" onclick={() => updaterStore.dismiss()}>Close</Button>
			</Dialog.Footer>
		{:else if updaterStore.status === 'error'}
			<div class="flex flex-col items-center gap-3 py-6">
				<AlertCircleIcon class="size-8 text-destructive" />
				<p class="text-sm text-center text-muted-foreground">
					{updaterStore.error ?? 'An unknown error occurred.'}
				</p>
			</div>

			<Dialog.Footer>
				<Button variant="ghost" onclick={() => updaterStore.dismiss()}>Close</Button>
				<Button onclick={() => updaterStore.manualCheck()}>Retry</Button>
			</Dialog.Footer>
		{/if}
	</Dialog.Content>
</Dialog.Root>
