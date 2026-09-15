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
	import SettingsToggle from './SettingsToggle.svelte';
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

<div class="space-y-6">
	<div>
		<h2 class="text-sm font-semibold">Server mode</h2>
		<p class="mt-1 text-xs text-muted-foreground">
			Run a control-plane server on this machine so other devices can create worktrees and spawn
			<code>claude remote-control</code> sessions here. Secure it with a private network (e.g. Tailscale).
			Spawned sessions appear in the Claude mobile app automatically.
		</p>
	</div>

	<SettingsToggle
		label="Enable server mode"
		description="Start the embedded Workbench server."
		checked={store.serverMode}
		onCheckedChange={toggleServerMode}
	/>

	<div class="flex items-center justify-between gap-4">
		<div>
			<p class="text-sm font-medium">Port</p>
			<p class="text-xs text-muted-foreground">TCP port the server listens on.</p>
		</div>
		<Input
			type="number"
			min="1"
			max="65535"
			class="w-28"
			value={store.serverPort}
			onchange={(e) => applyServerPort((e.currentTarget as HTMLInputElement).value)}
		/>
	</div>

	{#if store.serverToken}
		<div class="space-y-2">
			<div>
				<p class="text-sm font-medium">Token</p>
				<p class="text-xs text-muted-foreground">
					Every client (phone, remote desktop) must present this token.
				</p>
			</div>
			<div class="flex items-center gap-1">
				<code class="min-w-0 flex-1 truncate rounded bg-muted px-2 py-1.5 font-mono text-xs">
					{tokenRevealed ? store.serverToken : '•'.repeat(32)}
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
					Regenerate
				</Button>
			</div>
		</div>
	{/if}

	<p class="text-xs text-wb-warn">
		Traffic, including the token, is plaintext HTTP. Only enable server mode on a private network
		such as Tailscale.
	</p>

	<div class="flex items-center justify-between gap-4">
		<p
			class="text-xs"
			class:text-wb-ok={server.running}
			class:text-muted-foreground={!server.running}
		>
			{server.running && server.address ? `Listening on ${server.address}` : 'Not running'}
		</p>
		<Button variant="outline" size="sm" disabled={!server.running} onclick={openPairing}>
			Pair phone
		</Button>
	</div>
	{#if serverError}
		<p class="text-xs text-wb-err">{serverError}</p>
	{/if}
</div>

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
