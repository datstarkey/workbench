<script lang="ts">
	import { onMount } from 'svelte';
	import { ControlPlaneSidebar } from '@workbench/control-plane-ui';
	import { Button } from '@workbench/ui/button';
	import { Input } from '@workbench/ui/input';
	import Terminal from './Terminal.svelte';
	import { MobileClient } from './client.svelte.ts';

	const c = new MobileClient();

	// Remember the last server: auto-reconnect on launch if one was saved.
	onMount(() => {
		if (c.hasSavedServer) void c.connect();
	});
</script>

{#if c.activeTerminal && c.store}
	{#key c.activeTerminal.id}
		<Terminal
			serverUrl={c.url}
			token={c.token}
			id={c.activeTerminal.id}
			name={c.activeTerminal.name ?? 'terminal'}
			onClose={c.closeTerminal}
		/>
	{/key}
{:else}
	<div class="flex h-full flex-col bg-wb-bg text-wb-ink">
		<header
			class="flex shrink-0 items-center gap-2 border-b border-wb-hair bg-wb-rail px-3"
			style="padding-top: env(safe-area-inset-top); height: calc(2.75rem + env(safe-area-inset-top));"
		>
			<span class="text-[13px] font-semibold tracking-tight">Workbench</span>
			{#if c.store}
				<span class="min-w-0 flex-1 truncate font-mono text-[11px] text-wb-ink-soft"
					>{c.serverLabel}</span
				>
				<button
					class="rounded px-2 py-1 text-[11px] text-wb-ink-mute transition-colors hover:bg-wb-panel2 hover:text-wb-ink"
					onclick={() => c.refreshAll()}
				>
					Refresh
				</button>
				<button
					class="rounded px-2 py-1 text-[11px] text-wb-ink-mute transition-colors hover:bg-wb-panel2 hover:text-wb-ink"
					onclick={() => c.disconnect()}
				>
					Disconnect
				</button>
			{/if}
		</header>

		<main class="flex min-h-0 flex-1 flex-col overflow-hidden bg-wb-panel">
			{#if c.store}
				{#if c.terminals.length}
					<div class="shrink-0 border-b border-wb-hair p-3">
						<h2 class="mb-2 text-[11px] font-semibold tracking-wider text-wb-ink-soft uppercase">
							Terminals
						</h2>
						<div class="flex flex-col gap-1.5">
							{#each c.terminals as t (t.id)}
								<div
									class="flex items-center gap-2 rounded-md border border-wb-hair bg-wb-panel2 p-2"
								>
									<button
										class="min-w-0 flex-1 truncate text-left text-sm text-wb-ink"
										onclick={() => c.selectTerminal(t.id)}
									>
										{t.name ?? t.id.slice(0, 8)}
										{#if !t.alive}<span class="font-mono text-[10px] text-wb-ink-soft"
												>(exited)</span
											>{/if}
									</button>
									<button
										class="rounded px-2 py-0.5 text-xs text-wb-ink-mute transition-colors hover:bg-wb-bg hover:text-wb-err"
										onclick={() => c.killTerminal(t.id)}
									>
										✕
									</button>
								</div>
							{/each}
						</div>
					</div>
				{/if}
				<div class="min-h-0 flex-1 overflow-hidden">
					<ControlPlaneSidebar store={c.store} onOpenTerminal={c.createTerminal} />
				</div>
			{:else}
				<div class="flex h-full items-start justify-center overflow-y-auto p-5">
					<div class="mt-8 w-full max-w-sm rounded-lg border border-wb-hair bg-wb-panel2 p-4">
						<h1 class="text-sm font-semibold">Connect to server</h1>
						<p class="mb-4 font-mono text-[11px] text-wb-ink-soft">
							workbench-server control plane
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
							Token <span class="text-wb-ink-soft">(optional)</span>
						</label>
						<Input
							id="tok"
							type="password"
							bind:value={c.token}
							placeholder="bearer token"
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
			{/if}
		</main>
	</div>
{/if}
