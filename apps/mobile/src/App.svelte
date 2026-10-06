<script lang="ts">
	import { onMount } from 'svelte';
	import { watch } from 'runed';
	import { Button } from '@workbench/ui/button';
	import { Input } from '@workbench/ui/input';
	import ChatScreen from './ChatScreen.svelte';
	import Home from './Home.svelte';
	import MachineList from './MachineList.svelte';
	import Terminal from './Terminal.svelte';
	import ScanOverlay from './ScanOverlay.svelte';
	import { MobileClient } from './client.svelte.ts';
	import UpdateBanner from './UpdateBanner.svelte';
	import { AppUpdater } from './app-updater.svelte.ts';

	const c = new MobileClient();
	const updater = new AppUpdater();

	watch(
		() => [c.notifications.enabled, c.connection, c.machineId, c.machine?.name],
		() => {
			void c.notifications.configure(
				c.connection && c.machineId
					? { ...c.connection, machineId: c.machineId, name: c.machine?.name ?? 'Workbench' }
					: null
			);
		}
	);

	// Remember the last server: auto-reconnect on launch if one was saved.
	onMount(() => {
		if (c.hasSavedServer) void c.connect();
		const stopWatching = c.watch();
		let destroyed = false;
		let removeAlerts = () => {};
		void c.notifications
			.listen(async (machineId, chat) => {
				if (!c.machines.list.some((m) => m.id === machineId)) return;
				if (c.machineId !== machineId) await c.switchTo(machineId);
				if (!destroyed && c.machineId === machineId) c.openChat(c.chatRef(chat));
			})
			.then((remove) => {
				if (destroyed) remove();
				else removeAlerts = remove;
			});
		return () => {
			destroyed = true;
			stopWatching();
			removeAlerts();
		};
	});
</script>

{#if c.scanning}
	<ScanOverlay onCancel={c.cancelScan} />
{:else if c.activeChat && c.store}
	{#key c.chatScreenKey}
		<ChatScreen client={c} ref={c.activeChat} />
	{/key}
{:else if c.activeTerminal && c.store}
	{@const terminalId = c.activeTerminal.id}
	{#key terminalId}
		<Terminal
			serverUrl={c.connection?.url ?? ''}
			token={c.connection?.token ?? ''}
			id={terminalId}
			name={c.activeTerminal.name ?? 'terminal'}
			onClose={c.closeTerminal}
			onShowChat={c.terminalChats[terminalId] ? () => c.showAsChat(terminalId) : undefined}
			switching={c.switching}
			notice={c.notice}
		/>
	{/key}
{:else if c.store}
	<div class="flex h-full flex-col">
		<div class="min-h-0 flex-1"><Home client={c} /></div>
		<UpdateBanner {updater} />
	</div>
{:else}
	<div class="flex h-full flex-col bg-wb-bg text-wb-ink">
		<header
			class="flex shrink-0 items-center gap-2 border-b border-wb-hair bg-wb-rail px-4"
			style="padding-top: env(safe-area-inset-top); height: calc(3rem + env(safe-area-inset-top));"
		>
			<span class="text-[15px] font-semibold tracking-tight">Workbench</span>
		</header>

		<main class="flex min-h-0 flex-1 flex-col overflow-hidden bg-wb-panel">
			<div class="flex h-full flex-col items-center overflow-y-auto p-5">
				{#if c.machines.list.length > 0}
					<section class="mt-3 flex w-full max-w-sm flex-col gap-2">
						<h2 class="text-sm font-semibold">Saved machines</h2>
						<MachineList client={c} />
					</section>
				{/if}
				<div class="mt-8 w-full max-w-sm rounded-lg border border-wb-hair bg-wb-panel2 p-4">
					<h1 class="text-sm font-semibold">
						{c.machines.list.length > 0 ? 'Add a machine' : 'Connect to server'}
					</h1>
					<p class="mb-4 font-mono text-[11px] text-wb-ink-soft">workbench-server control plane</p>

					<Button
						onclick={() => c.scanAndConnect()}
						disabled={c.scanning || c.connecting}
						class="mb-2 w-full"
					>
						{c.scanning ? 'Scanning…' : 'Scan QR code'}
					</Button>
					<p class="mb-4 text-center text-[11px] text-wb-ink-soft">
						Desktop: Settings → Server mode → Pair phone. Or enter the details below.
					</p>

					<label class="mb-1 block text-[11px] font-medium text-wb-ink-mute" for="srv"
						>Server (Tailscale IP)</label
					>
					<Input
						id="srv"
						bind:value={c.url}
						placeholder="100.x.x.x"
						autocapitalize="off"
						autocorrect="off"
						spellcheck={false}
						class="mb-3 font-mono"
					/>

					<label class="mb-1 block text-[11px] font-medium text-wb-ink-mute" for="tok">
						Token
					</label>
					<Input
						id="tok"
						type="password"
						bind:value={c.token}
						placeholder="from Settings → Server mode"
						autocapitalize="off"
						autocorrect="off"
						spellcheck={false}
						class="mb-4 font-mono"
					/>

					<Button onclick={() => c.connect()} disabled={c.connecting} class="w-full">
						{c.connecting ? 'Connecting…' : 'Connect'}
					</Button>

					{#if c.connectError}
						<p class="mt-3 font-mono text-[11px] text-wb-err">{c.connectError}</p>
					{/if}
				</div>
			</div>
		</main>
		<UpdateBanner {updater} />
	</div>
{/if}
