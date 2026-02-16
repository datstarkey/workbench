<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Badge } from '$lib/components/ui/badge';
	import { getTrelloStore } from '$stores/context';
	import LoaderIcon from '@lucide/svelte/icons/loader';
	import CheckIcon from '@lucide/svelte/icons/check';
	import XIcon from '@lucide/svelte/icons/x';
	import { onMount } from 'svelte';

	const trelloStore = getTrelloStore();

	let apiKey = $state('');
	let token = $state('');
	let validating = $state(false);
	let disconnecting = $state(false);
	let validationResult = $state<boolean | null>(null);

	onMount(() => {
		trelloStore.loadCredentials();
	});

	async function handleValidate() {
		if (!apiKey.trim() || !token.trim()) return;
		validating = true;
		validationResult = null;
		try {
			const valid = await trelloStore.validateAuth(apiKey.trim(), token.trim());
			validationResult = valid;
			if (valid) {
				await trelloStore.saveCredentials(apiKey.trim(), token.trim());
				apiKey = '';
				token = '';
			}
		} catch {
			validationResult = false;
		} finally {
			validating = false;
		}
	}

	async function handleDisconnect() {
		disconnecting = true;
		try {
			await trelloStore.disconnect();
		} finally {
			disconnecting = false;
		}
	}
</script>

<div class="space-y-3">
	{#if trelloStore.authenticated}
		<div class="flex items-center justify-between">
			<div class="flex items-center gap-2">
				<h4 class="text-xs font-medium">Authentication</h4>
				<Badge variant="secondary" class="h-4 gap-1 text-[10px]">
					<CheckIcon class="size-2.5" />
					Connected
				</Badge>
			</div>
			<Button
				variant="outline"
				size="sm"
				class="h-7 text-xs text-destructive hover:bg-destructive/10"
				onclick={handleDisconnect}
				disabled={disconnecting}
			>
				{#if disconnecting}
					<LoaderIcon class="mr-1 size-3 animate-spin" />
				{/if}
				Disconnect
			</Button>
		</div>
	{:else}
		<h4 class="text-xs font-medium">Authentication</h4>

		<div class="space-y-2">
			<div>
				<label
					class="mb-1 block text-[10px] font-medium text-muted-foreground"
					for="trello-api-key"
				>
					API Key
				</label>
				<Input
					id="trello-api-key"
					type="text"
					class="h-8 text-xs"
					placeholder="Your Trello API key"
					bind:value={apiKey}
				/>
			</div>
			<div>
				<label class="mb-1 block text-[10px] font-medium text-muted-foreground" for="trello-token">
					Token
				</label>
				<Input
					id="trello-token"
					type="text"
					class="h-8 text-xs"
					placeholder="Your Trello token"
					bind:value={token}
				/>
			</div>
		</div>

		<div class="flex items-center gap-2">
			<Button
				size="sm"
				class="h-7 text-xs"
				onclick={handleValidate}
				disabled={validating || !apiKey.trim() || !token.trim()}
			>
				{#if validating}
					<LoaderIcon class="mr-1 size-3 animate-spin" />
				{/if}
				Connect
			</Button>
			{#if validationResult === false}
				<span class="flex items-center gap-1 text-[10px] text-destructive">
					<XIcon class="size-3" />
					Invalid credentials
				</span>
			{/if}
		</div>

		<p class="text-[10px] text-muted-foreground">
			Get your API key and token from
			<button
				type="button"
				class="underline"
				onclick={() =>
					import('@tauri-apps/api/core').then((m) =>
						m.invoke('open_url', { url: 'https://trello.com/power-ups/admin' })
					)}
			>
				trello.com/power-ups/admin
			</button>
		</p>
	{/if}
</div>
