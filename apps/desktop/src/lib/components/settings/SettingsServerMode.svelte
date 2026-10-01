<script lang="ts">
	import CheckIcon from '@lucide/svelte/icons/check';
	import CopyIcon from '@lucide/svelte/icons/copy';
	import EyeIcon from '@lucide/svelte/icons/eye';
	import EyeOffIcon from '@lucide/svelte/icons/eye-off';
	import { onMount } from 'svelte';
	import { emit } from '@tauri-apps/api/event';
	import { Button } from '@workbench/ui/button';
	import { Input } from '@workbench/ui/input';
	import ConfirmDialog from '$components/ConfirmDialog.svelte';
	import { ConfirmAction } from '$lib/utils/confirm-action.svelte';
	import {
		pairingAddresses,
		startServer,
		stopServer,
		serverStatus,
		type PairingAddress,
		type ServerStatus
	} from '$lib/server-mode';
	import { getWorkbenchSettingsStore } from '$stores/context';
	import { Switch } from '@workbench/ui/switch';
	import SettingsNote from './SettingsNote.svelte';
	import SettingsRow from './SettingsRow.svelte';
	import SettingsSection from './SettingsSection.svelte';
	import ServerPairingDialog from './ServerPairingDialog.svelte';
	import { boundPort } from './server-pairing';

	const store = getWorkbenchSettingsStore();

	const stoppedServer: ServerStatus = { running: false, address: null, token: null };
	let server: ServerStatus = $state(stoppedServer);
	let serverError: string | null = $state(null);
	let tokenRevealed = $state(false);
	let tokenCopied = $state(false);
	const tokenRotation = new ConfirmAction<true>();
	let pairing = $state<{ addresses: PairingAddress[]; port: number } | null>(null);
	let pairingOpen = $state(false);

	onMount(async () => {
		try {
			server = await serverStatus();
		} catch {
			/* leave as not-running */
		}
	});

	async function toggleServerMode(checked: boolean) {
		serverError = null;
		store.set('serverMode', checked);
		try {
			const token = checked ? await store.ensureServerToken() : null;
			await store.save();
			// The main window saves settings too; keep its copy of the token current.
			await emit('settings:changed');
			server = token ? await startServer(store.serverPort, token) : await stopServer();
		} catch (e) {
			serverError = e instanceof Error ? e.message : String(e);
			store.set('serverMode', false);
			await store.save();
		}
	}

	/** Restart a running LAN server so a new port takes effect. */
	async function restartServer() {
		serverError = null;
		try {
			// Ask Rust, not the status cached at mount, whether it's running.
			if (!(await serverStatus()).running) return;
			await stopServer();
			server = await startServer(store.serverPort, await store.ensureServerToken());
		} catch (e) {
			serverError = e instanceof Error ? e.message : String(e);
			server = stoppedServer;
		}
	}

	async function applyServerPort(value: string) {
		const port = Number(value);
		if (!Number.isInteger(port) || port < 1 || port > 65535) return;
		store.set('serverPort', port);
		await store.save();
		await restartServer();
	}

	/** Rust saves the new token and restarts a running server, cutting off old clients. */
	async function rotateToken() {
		await store.rotateServerToken();
	}

	async function openPairing() {
		serverError = null;
		try {
			// The QR must carry the port the server is actually bound to right now.
			server = await serverStatus();
			const port = boundPort(server.address);
			if (!server.running || port === null) {
				serverError = 'Start server mode to pair a phone.';
				return;
			}
			pairing = { addresses: await pairingAddresses(), port };
			pairingOpen = true;
		} catch (e) {
			serverError = e instanceof Error ? e.message : String(e);
		}
	}

	async function copyServerToken() {
		if (!store.serverToken) return;
		await navigator.clipboard.writeText(store.serverToken);
		tokenCopied = true;
		setTimeout(() => (tokenCopied = false), 1500);
	}
</script>

{#snippet status()}
	<span class="flex items-center gap-2">
		<span class={['size-2 rounded-full', server.running ? 'bg-wb-ok' : 'bg-wb-ink-soft']}></span>
		{#if server.running && server.address}
			Listening on <code class="text-wb-ink">{server.address}</code>
		{:else}
			Not running
		{/if}
	</span>
{/snippet}

<SettingsSection>
	<SettingsRow label="Server mode" description={status}>
		{#snippet control()}
			<Button variant="outline" size="sm" disabled={!server.running} onclick={openPairing}>
				Pair phone
			</Button>
			<Switch
				checked={store.serverMode}
				onCheckedChange={toggleServerMode}
				aria-label="Server mode"
			/>
		{/snippet}
		{#if serverError}
			<p class="mt-2 text-xs text-wb-err">{serverError}</p>
		{/if}
	</SettingsRow>
</SettingsSection>

<SettingsSection title="Connection">
	<SettingsRow label="Port" description="Restarts a running server when changed.">
		{#snippet control()}
			<Input
				type="number"
				min="1"
				max="65535"
				class="h-8 w-24 font-mono text-xs"
				aria-label="Port"
				value={store.serverPort}
				onchange={(e) => applyServerPort((e.currentTarget as HTMLInputElement).value)}
			/>
		{/snippet}
	</SettingsRow>

	{#if store.serverToken}
		<SettingsRow label="Token" description="Every phone or remote desktop must present it.">
			{#snippet control()}
				<code
					class="flex h-8 w-44 items-center truncate rounded-md border border-wb-hair bg-wb-bg px-2.5 font-mono text-xs"
				>
					{tokenRevealed ? store.serverToken : '•'.repeat(24)}
				</code>
				<Button
					variant="ghost"
					size="icon-sm"
					aria-label={tokenRevealed ? 'Hide token' : 'Reveal token'}
					onclick={() => (tokenRevealed = !tokenRevealed)}
				>
					{#if tokenRevealed}<EyeOffIcon />{:else}<EyeIcon />{/if}
				</Button>
				<Button variant="ghost" size="icon-sm" aria-label="Copy token" onclick={copyServerToken}>
					{#if tokenCopied}<CheckIcon />{:else}<CopyIcon />{/if}
				</Button>
				<Button variant="outline" size="sm" onclick={() => tokenRotation.request(true)}>
					Regenerate…
				</Button>
			{/snippet}
		</SettingsRow>
	{/if}
</SettingsSection>

<SettingsNote tone="warn">
	Traffic, including the token, is plain HTTP. Only turn this on over a private network such as
	Tailscale. Sessions started from another device show up in the Claude mobile app.
</SettingsNote>

{#if pairing}
	<ServerPairingDialog bind:open={pairingOpen} addresses={pairing.addresses} port={pairing.port} />
{/if}

<ConfirmDialog
	bind:open={tokenRotation.open}
	title="Regenerate server token?"
	description="Devices using the current token lose access and must be given the new one."
	confirmLabel="Regenerate"
	destructive
	error={tokenRotation.error}
	onConfirm={() => tokenRotation.confirm(rotateToken)}
/>
