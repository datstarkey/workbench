<script lang="ts">
	import * as Dialog from '@workbench/ui/dialog';
	import { Button } from '@workbench/ui/button';
	import { Input } from '@workbench/ui/input';
	import { createHttpTransport } from '@workbench/transport';
	import { ControlPlaneStore, ControlPlaneSidebar } from '@workbench/control-plane-ui';

	let { open = $bindable(false) }: { open?: boolean } = $props();

	const URL_KEY = 'workbench.remoteUrl';
	const TOKEN_KEY = 'workbench.remoteToken';

	let url = $state(localStorage.getItem(URL_KEY) ?? 'http://localhost:4317');
	let token = $state(localStorage.getItem(TOKEN_KEY) ?? '');
	let store = $state<ControlPlaneStore | null>(null);

	async function connect() {
		const baseUrl = url.trim().replace(/\/$/, '');
		if (!baseUrl) return;
		localStorage.setItem(URL_KEY, baseUrl);
		localStorage.setItem(TOKEN_KEY, token.trim());
		const transport = createHttpTransport({
			baseUrl,
			token: token.trim() || undefined
		});
		const s = new ControlPlaneStore(transport);
		store = s;
		await s.refresh();
	}

	function disconnect() {
		store = null;
	}
</script>

<Dialog.Root bind:open onOpenChange={(o) => !o && disconnect()}>
	<Dialog.Content class="max-w-md">
		<Dialog.Header>
			<Dialog.Title>Remote server</Dialog.Title>
			<Dialog.Description>
				Connect to a Workbench server to spawn sessions and manage worktrees on another machine.
			</Dialog.Description>
		</Dialog.Header>

		{#if !store}
			<div class="flex flex-col gap-3 py-2">
				<label class="flex flex-col gap-1 text-sm">
					Server URL
					<Input bind:value={url} placeholder="http://host:4317" />
				</label>
				<label class="flex flex-col gap-1 text-sm">
					Token (optional)
					<Input bind:value={token} type="password" placeholder="bearer token" />
				</label>
				<Button onclick={connect}>Connect</Button>
			</div>
		{:else}
			<div class="flex items-center justify-between py-1">
				<span class="truncate text-xs text-muted-foreground">{url}</span>
				<Button variant="ghost" size="sm" onclick={disconnect}>Disconnect</Button>
			</div>
			<div class="h-[60vh]">
				<ControlPlaneSidebar {store} />
			</div>
		{/if}
	</Dialog.Content>
</Dialog.Root>
