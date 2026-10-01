<script lang="ts">
	import type { Attachment } from 'svelte/attachments';
	import { Terminal } from '@xterm/xterm';
	import { FitAddon } from '@xterm/addon-fit';
	import '@xterm/xterm/css/xterm.css';
	import { terminalWsUrl } from './terminal-url.ts';
	import { touchScroll } from './touch-scroll.ts';
	import ViewSwitch from './ViewSwitch.svelte';
	import {
		reconnectDelay,
		statusForTextFrame,
		statusOnClose,
		type TerminalStatus
	} from './terminal-status.ts';

	let {
		serverUrl,
		token,
		id,
		name,
		onClose,
		onShowChat,
		switching = false
	}: {
		serverUrl: string;
		token?: string;
		id: string;
		name: string;
		onClose: () => void;
		/** Set when this terminal runs a Claude conversation that can move to chat. */
		onShowChat?: () => void;
		switching?: boolean;
	} = $props();

	let status = $state<TerminalStatus>('connecting');
	let ws: WebSocket | undefined;
	/** Re-attach after a takeover, kicking the device that holds the terminal now. */
	let takeControl = () => {};

	// Android soft keyboards lack arrows / Esc / Tab / Ctrl — provide them here.
	const KEYS: { label: string; seq: string }[] = [
		{ label: 'Esc', seq: '\x1b' },
		{ label: 'Tab', seq: '\t' },
		{ label: '⇧Tab', seq: '\x1b[Z' },
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

	const mountTerminal: Attachment<HTMLDivElement> = (host) => {
		const term = new Terminal({
			cursorBlink: true,
			fontSize: 13,
			fontFamily: 'ui-monospace, SFMono-Regular, Menlo, monospace',
			theme: { background: '#0f1115', foreground: '#e7eaf0' }
		});
		const fit = new FitAddon();
		term.loadAddon(fit);
		term.open(host);
		fit.fit();

		let retry: ReturnType<typeof setTimeout> | undefined;
		let attempt = 0;

		// Attach to the persistent session; the server replays scrollback first.
		const attach = () => {
			clearTimeout(retry);
			status = 'connecting';
			let replayed = false;
			const socket = new WebSocket(terminalWsUrl(serverUrl, id, token));
			ws = socket;
			socket.binaryType = 'arraybuffer';

			socket.onopen = () => {
				status = 'open';
				attempt = 0;
				term.focus();
				socket.send(JSON.stringify({ t: 'r', c: term.cols, r: term.rows }));
			};
			socket.onmessage = (ev) => {
				if (ev.data instanceof ArrayBuffer) {
					// A re-attach replays the whole scrollback; clear the old copy first.
					if (!replayed) term.reset();
					replayed = true;
					term.write(new Uint8Array(ev.data));
				} else {
					status = statusForTextFrame(ev.data) ?? status;
				}
			};
			socket.onclose = () => {
				status = statusOnClose(status);
				// Hidden (phone locked): wait for `resume` instead of retrying in the dark.
				if (status === 'reconnecting' && !document.hidden)
					retry = setTimeout(attach, reconnectDelay(attempt++));
			};
		};
		const reattach = () => {
			if (ws) ws.onopen = ws.onmessage = ws.onclose = null;
			ws?.close();
			attempt = 0;
			attach();
		};
		takeControl = reattach;
		attach();

		// After a long sleep the socket can still read OPEN while the server has
		// dropped it, so re-attach rather than trust it.
		let hiddenAt = 0;
		const resume = () => {
			if (document.hidden) {
				hiddenAt = Date.now();
				return;
			}
			const slept = Date.now() - hiddenAt > 10_000;
			if (status === 'reconnecting' || (slept && status === 'open')) reattach();
		};
		document.addEventListener('visibilitychange', resume);

		term.onData((d) => send(d));
		term.onResize(({ cols, rows }) => {
			if (ws && ws.readyState === WebSocket.OPEN)
				ws.send(JSON.stringify({ t: 'r', c: cols, r: rows }));
		});

		const ro = new ResizeObserver(() => fit.fit());
		ro.observe(host);

		// Re-fit when the soft keyboard shows/hides (visualViewport shrinks).
		const vv = window.visualViewport;
		const onVv = () => fit.fit();
		vv?.addEventListener('resize', onVv);

		return () => {
			ro.disconnect();
			vv?.removeEventListener('resize', onVv);
			document.removeEventListener('visibilitychange', resume);
			clearTimeout(retry);
			// Detach the socket handlers before disposing the terminal so a frame that
			// arrives during teardown can't call term.write() on a disposed terminal.
			// Closing the socket only detaches — the server keeps the shell alive.
			if (ws) {
				ws.onopen = ws.onmessage = ws.onclose = null;
				ws.close();
			}
			ws = undefined;
			term.dispose();
		};
	};
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
		{#if status === 'taken_over'}
			<span class="text-[10px] text-wb-warn">Opened on another device</span>
			<button
				class="rounded bg-wb-panel2 px-2 py-1 text-[11px] text-wb-ink active:bg-wb-panel"
				onclick={() => takeControl()}
			>
				Take control
			</button>
		{:else}
			<span
				class="text-[10px] uppercase"
				class:text-wb-ok={status === 'open'}
				class:text-wb-ink-soft={status === 'connecting' || status === 'reconnecting'}
				class:text-wb-err={status === 'revoked' || status === 'exited'}
			>
				{status}
			</span>
		{/if}
		{#if onShowChat}
			<ViewSwitch view="terminal" disabled={switching} onSwitch={onShowChat} />
		{/if}
	</header>
	{#if onShowChat}
		<div
			class="flex shrink-0 items-center gap-3 border-b border-wb-hair-soft bg-wb-panel px-3 py-2 text-xs text-wb-ink-mute"
		>
			Same conversation in the Claude CLI
			<button
				type="button"
				class="ml-auto rounded-lg border border-wb-hair bg-wb-panel2 px-3 py-1.5 font-medium text-wb-ink active:bg-wb-bg disabled:opacity-50"
				disabled={switching}
				onclick={onShowChat}
			>
				Back to chat
			</button>
		</div>
	{/if}

	<div
		{@attach mountTerminal}
		{@attach touchScroll}
		class="min-h-0 flex-1 overflow-hidden p-1"
	></div>

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
