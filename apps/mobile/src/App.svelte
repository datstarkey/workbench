<script lang="ts">
	import { onMount } from 'svelte';
	import { ControlPlaneStore, ControlPlaneSidebar } from '@workbench/control-plane-ui';
	import { createHttpTransport } from '@workbench/transport';
	import { Button } from '@workbench/ui/button';
	import { Input } from '@workbench/ui/input';
	import Terminal from './Terminal.svelte';

	const LS_URL = 'wb.serverUrl';
	const LS_TOKEN = 'wb.token';
	const DEFAULT_PORT = '4317';

	type TerminalMeta = {
		id: string;
		name?: string;
		cwd: string;
		createdAt: number;
		alive: boolean;
	};

	// localStorage can throw in some webview contexts — never let it crash mount.
	function lsGet(key: string): string | null {
		try {
			return localStorage.getItem(key);
		} catch {
			return null;
		}
	}
	function lsSet(key: string, value: string) {
		try {
			localStorage.setItem(key, value);
		} catch {
			/* ignore */
		}
	}

	let url = $state(lsGet(LS_URL) ?? '');
	let token = $state(lsGet(LS_TOKEN) ?? '');
	let store = $state<ControlPlaneStore | null>(null);
	let connecting = $state(false);
	let connectError = $state<string | null>(null);
	let terminals = $state<TerminalMeta[]>([]);
	let activeTerminalId = $state<string | null>(null);

	let serverLabel = $derived(url.replace(/^https?:\/\//, ''));
	let activeTerminal = $derived(terminals.find((t) => t.id === activeTerminalId) ?? null);

	// Accept a bare Tailscale IP / host: add http:// and the default port so you
	// can just paste the IP.
	function normalizeUrl(raw: string): string {
		let s = raw.trim();
		if (!s) return s;
		if (!/^https?:\/\//.test(s)) s = `http://${s}`;
		try {
			const u = new URL(s);
			if (!u.port) u.port = DEFAULT_PORT;
			return u.toString().replace(/\/$/, '');
		} catch {
			return s.replace(/\/$/, '');
		}
	}

	function authHeaders(): Record<string, string> {
		return token ? { authorization: `Bearer ${token}` } : {};
	}

	async function connect() {
		connecting = true;
		connectError = null;
		try {
			const base = normalizeUrl(url);
			if (!base) throw new Error('enter a server address');
			const res = await fetch(`${base}/health`, { headers: authHeaders() });
			if (!res.ok) throw new Error(`health check returned ${res.status}`);

			url = base;
			lsSet(LS_URL, base);
			lsSet(LS_TOKEN, token);

			const transport = createHttpTransport({ baseUrl: base, token: token || undefined });
			const next = new ControlPlaneStore(transport);
			await next.refresh();
			store = next;
			await refreshTerminals();
		} catch (e) {
			connectError = e instanceof Error ? e.message : String(e);
		} finally {
			connecting = false;
		}
	}

	function disconnect() {
		store?.dispose();
		store = null;
		terminals = [];
		activeTerminalId = null;
	}

	async function refreshTerminals() {
		if (!store) return;
		try {
			const res = await fetch(`${url}/remote/terminals`, { headers: authHeaders() });
			if (res.ok) {
				const data = await res.json();
				// Guard the {#each terminals} below: a non-array body would throw at render.
				terminals = Array.isArray(data) ? data : [];
			}
		} catch {
			/* ignore */
		}
	}

	async function createTerminal(
		projectPath: string,
		worktreePath: string | undefined,
		name: string,
		command?: string
	) {
		if (!store) return;
		try {
			const res = await fetch(`${url}/remote/terminals`, {
				method: 'POST',
				headers: { 'content-type': 'application/json', ...authHeaders() },
				body: JSON.stringify({ projectPath, worktreePath, name, command, cols: 80, rows: 24 })
			});
			if (!res.ok) throw new Error(`create terminal failed (${res.status})`);
			const meta: TerminalMeta = await res.json();
			// Open the new terminal immediately, then let refreshTerminals() reconcile
			// with the server list — otherwise a list that hasn't yet surfaced the new
			// id leaves `activeTerminal` null and the view never opens.
			if (!terminals.some((t) => t.id === meta.id)) terminals = [...terminals, meta];
			activeTerminalId = meta.id;
			await refreshTerminals();
		} catch (e) {
			connectError = e instanceof Error ? e.message : String(e);
		}
	}

	async function killTerminal(id: string) {
		try {
			await fetch(`${url}/remote/terminals/${id}`, { method: 'DELETE', headers: authHeaders() });
		} catch {
			/* ignore */
		}
		if (activeTerminalId === id) activeTerminalId = null;
		await refreshTerminals();
	}

	function closeTerminal() {
		activeTerminalId = null;
		void refreshTerminals();
	}

	// Remember the last server: auto-reconnect on launch if one was saved.
	onMount(() => {
		if (lsGet(LS_URL)) connect();
	});
</script>

{#if activeTerminal && store}
	{#key activeTerminal.id}
		<Terminal
			serverUrl={url}
			{token}
			id={activeTerminal.id}
			name={activeTerminal.name ?? 'terminal'}
			onClose={closeTerminal}
		/>
	{/key}
{:else}
	<div class="flex h-full flex-col bg-wb-bg text-wb-ink">
		<header
			class="flex shrink-0 items-center gap-2 border-b border-wb-hair bg-wb-rail px-3"
			style="padding-top: env(safe-area-inset-top); height: calc(2.75rem + env(safe-area-inset-top));"
		>
			<span class="text-[13px] font-semibold tracking-tight">Workbench</span>
			{#if store}
				<span class="min-w-0 flex-1 truncate font-mono text-[11px] text-wb-ink-soft"
					>{serverLabel}</span
				>
				<button
					class="rounded px-2 py-1 text-[11px] text-wb-ink-mute transition-colors hover:bg-wb-panel2 hover:text-wb-ink"
					onclick={() => {
						store?.refresh();
						refreshTerminals();
					}}
				>
					Refresh
				</button>
				<button
					class="rounded px-2 py-1 text-[11px] text-wb-ink-mute transition-colors hover:bg-wb-panel2 hover:text-wb-ink"
					onclick={disconnect}
				>
					Disconnect
				</button>
			{/if}
		</header>

		<main class="flex min-h-0 flex-1 flex-col overflow-hidden bg-wb-panel">
			{#if store}
				{#if terminals.length}
					<div class="shrink-0 border-b border-wb-hair p-3">
						<h2 class="mb-2 text-[11px] font-semibold tracking-wider text-wb-ink-soft uppercase">
							Terminals
						</h2>
						<div class="flex flex-col gap-1.5">
							{#each terminals as t (t.id)}
								<div
									class="flex items-center gap-2 rounded-md border border-wb-hair bg-wb-panel2 p-2"
								>
									<button
										class="min-w-0 flex-1 truncate text-left text-sm text-wb-ink"
										onclick={() => (activeTerminalId = t.id)}
									>
										{t.name ?? t.id.slice(0, 8)}
										{#if !t.alive}<span class="font-mono text-[10px] text-wb-ink-soft"
												>(exited)</span
											>{/if}
									</button>
									<button
										class="rounded px-2 py-0.5 text-xs text-wb-ink-mute transition-colors hover:bg-wb-bg hover:text-wb-err"
										onclick={() => killTerminal(t.id)}
									>
										✕
									</button>
								</div>
							{/each}
						</div>
					</div>
				{/if}
				<div class="min-h-0 flex-1 overflow-hidden">
					<ControlPlaneSidebar {store} onOpenTerminal={createTerminal} />
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
							bind:value={url}
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
							bind:value={token}
							placeholder="bearer token"
							class="mb-4 font-mono"
						/>

						<Button onclick={connect} disabled={connecting} class="w-full">
							{connecting ? 'Connecting…' : 'Connect'}
						</Button>

						{#if connectError}
							<p class="mt-3 font-mono text-[11px] text-wb-err">{connectError}</p>
						{/if}
					</div>
				</div>
			{/if}
		</main>
	</div>
{/if}
