<script lang="ts">
	import * as Dialog from '@workbench/ui/dialog';
	import * as Select from '@workbench/ui/select';
	import { getWorkbenchSettingsStore } from '$stores/context';
	import type { PairingAddress } from '$lib/server-mode';
	import { defaultPairingAddress, pairingQr, pairingUrl } from './server-pairing';

	let {
		open = $bindable(),
		addresses,
		port
	}: {
		open: boolean;
		addresses: PairingAddress[];
		/** The running LAN server's bound port. */
		port: number;
	} = $props();

	const store = getWorkbenchSettingsStore();

	let host = $derived(defaultPairingAddress(addresses));
	const tailscale = $derived(addresses.find((a) => a.address === host)?.tailscale ?? false);
	// Derived from the store, so a token rotated while the dialog is open redraws the code.
	const qr = $derived(host && store.serverToken ? pairingQr(host, port, store.serverToken) : null);
</script>

<Dialog.Root bind:open>
	<Dialog.Content class="sm:max-w-sm">
		<Dialog.Header>
			<Dialog.Title>Pair phone</Dialog.Title>
			<Dialog.Description>
				In the Workbench mobile app, tap “Scan QR code” and point the camera at this code.
			</Dialog.Description>
		</Dialog.Header>

		{#if !host}
			<p class="text-sm text-wb-err">No network address found. Connect this Mac to a network.</p>
		{:else if !qr}
			<p class="text-sm text-wb-err">Server mode has no token yet. Regenerate it in settings.</p>
		{:else}
			<Select.Root type="single" value={host} onValueChange={(v) => (host = v)}>
				<Select.Trigger class="w-full">
					{host}
				</Select.Trigger>
				<Select.Content>
					{#each addresses as a (a.address)}
						<Select.Item value={a.address}>
							{a.address}
							<span class="text-muted-foreground">
								{a.interface}{a.tailscale ? ' · Tailscale' : ''}
							</span>
						</Select.Item>
					{/each}
				</Select.Content>
			</Select.Root>

			<svg
				class="mx-auto size-56 rounded-md"
				viewBox="0 0 {qr.size} {qr.size}"
				shape-rendering="crispEdges"
				role="img"
				aria-label="Pairing QR code"
			>
				<rect width={qr.size} height={qr.size} fill="#fff" />
				<path d={qr.path} fill="#000" />
			</svg>

			<p class="text-center font-mono text-xs select-all">{pairingUrl(host, port)}</p>

			{#if !tailscale}
				<p class="text-xs text-wb-warn">
					Plain HTTP — anyone on this network could read the token. Prefer Tailscale.
				</p>
			{/if}
		{/if}
	</Dialog.Content>
</Dialog.Root>
