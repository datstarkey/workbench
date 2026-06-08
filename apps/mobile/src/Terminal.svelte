<script lang="ts">
	import { onMount } from 'svelte';
	import { Terminal } from '@xterm/xterm';
	import { FitAddon } from '@xterm/addon-fit';
	import '@xterm/xterm/css/xterm.css';

	let {
		serverUrl,
		id,
		name,
		onClose
	}: {
		serverUrl: string;
		id: string;
		name: string;
		onClose: () => void;
	} = $props();

	let host = $state<HTMLDivElement>();
	let status = $state<'connecting' | 'open' | 'closed'>('connecting');
	let ws: WebSocket | undefined;

	// Android soft keyboards lack arrows / Esc / Tab / Ctrl — provide them here.
	const KEYS: { label: string; seq: string }[] = [
		{ label: 'Esc', seq: '\x1b' },
		{ label: 'Tab', seq: '\t' },
		{ label: '^C', seq: '\x03' },
		{ label: '^D', seq: '\x04' },
		{ label: '←', seq: '\x1b[D' },
		{ label: '↑', seq: '\x1b[A' },
		{ label: '↓', seq: '\x1b[B' },
		{ label: '→', seq: '\x1b[C' }
	];

	function send(data: string) {
		if (ws && ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify({ t: 'i', d: data }));
	}

	onMount(() => {
		const term = new Terminal({
			cursorBlink: true,
			fontSize: 13,
			fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace',
			theme: { background: '#0f1115', foreground: '#e7eaf0' }
		});
		const fit = new FitAddon();
		term.loadAddon(fit);
		term.open(host!);
		fit.fit();

		// Attach to the persistent session; the server replays scrollback first.
		const base = serverUrl.replace(/^http/, 'ws').replace(/\/$/, '');
		const params = new URLSearchParams({ cols: String(term.cols), rows: String(term.rows) });
		ws = new WebSocket(`${base}/remote/terminals/${id}/ws?${params}`);
		ws.binaryType = 'arraybuffer';

		ws.onopen = () => {
			status = 'open';
			term.focus();
			ws?.send(JSON.stringify({ t: 'r', c: term.cols, r: term.rows }));
		};
		ws.onmessage = (ev) => {
			if (ev.data instanceof ArrayBuffer) term.write(new Uint8Array(ev.data));
			else term.write(ev.data);
		};
		ws.onclose = () => {
			status = 'closed';
		};

		term.onData((d) => send(d));
		term.onResize(({ cols, rows }) => {
			if (ws && ws.readyState === WebSocket.OPEN)
				ws.send(JSON.stringify({ t: 'r', c: cols, r: rows }));
		});

		const ro = new ResizeObserver(() => fit.fit());
		ro.observe(host!);

		// Re-fit when the soft keyboard shows/hides (visualViewport shrinks).
		const vv = window.visualViewport;
		const onVv = () => fit.fit();
		vv?.addEventListener('resize', onVv);

		return () => {
			ro.disconnect();
			vv?.removeEventListener('resize', onVv);
			// Closing the socket only detaches — the server keeps the shell alive.
			ws?.close();
			ws = undefined;
			term.dispose();
		};
	});
</script>

<div class="flex h-full flex-col bg-wb-bg">
	<header
		class="flex shrink-0 items-center gap-2 border-b border-wb-hair bg-wb-rail px-2"
		style="padding-top: env(safe-area-inset-top); height: calc(2.5rem + env(safe-area-inset-top));"
	>
		<button
			class="rounded px-2 py-1 text-[11px] text-wb-ink-mute transition-colors hover:bg-wb-panel2 hover:text-wb-ink"
			onclick={onClose}
		>
			← Back
		</button>
		<span class="min-w-0 flex-1 truncate font-mono text-[11px] text-wb-ink">{name}</span>
		<span
			class="text-[10px] uppercase"
			class:text-wb-ok={status === 'open'}
			class:text-wb-ink-soft={status === 'connecting'}
			class:text-wb-err={status === 'closed'}
		>
			{status}
		</span>
	</header>

	<div bind:this={host} class="min-h-0 flex-1 overflow-hidden p-1"></div>

	<!-- Extra keys row — sits above the soft keyboard. pointerdown+preventDefault
	     keeps focus on the terminal so tapping a key doesn't dismiss the keyboard. -->
	<div class="flex shrink-0 gap-1 overflow-x-auto border-t border-wb-hair bg-wb-rail px-2 py-1.5">
		{#each KEYS as k (k.label)}
			<button
				class="shrink-0 rounded border border-wb-hair bg-wb-panel2 px-3 py-1.5 font-mono text-xs text-wb-ink active:bg-wb-panel"
				onpointerdown={(e) => {
					e.preventDefault();
					send(k.seq);
				}}
			>
				{k.label}
			</button>
		{/each}
	</div>
</div>
