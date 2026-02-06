<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import { Terminal } from '@xterm/xterm';
	import { FitAddon } from '@xterm/addon-fit';
	import '@xterm/xterm/css/xterm.css';
	import type { ProjectConfig } from '$types/workbench';
	import { terminalOptions } from '$lib/terminal-config';
	import {
		createTerminal,
		writeTerminal,
		resizeTerminal,
		killTerminal,
		onTerminalData,
		onTerminalExit
	} from '$lib/hooks/useTerminal.svelte';

	let {
		sessionId,
		project,
		active,
		startupCommand
	}: {
		sessionId: string;
		project: ProjectConfig;
		active: boolean;
		startupCommand?: string;
	} = $props();

	let container: HTMLDivElement;
	let terminal: Terminal | null = null;
	let fitAddon: FitAddon | null = null;
	let unlistenData: (() => void) | null = null;
	let unlistenExit: (() => void) | null = null;
	let resizeObserver: ResizeObserver | null = null;
	let exited = false;
	let terminalError = $state('');
	let resizeTimeout: ReturnType<typeof setTimeout> | null = null;

	// Buffer early output to detect Claude CLI errors for auto-retry
	let earlyOutput = '';
	let claudeRetryCmd = '';

	function detectClaudeRetry(data: string): void {
		if (!startupCommand?.startsWith('claude') || claudeRetryCmd) return;
		earlyOutput += data;
		// Stop buffering after ~2KB
		if (earlyOutput.length > 2048) {
			earlyOutput = '';
			return;
		}
		const plain = earlyOutput.replace(new RegExp(String.raw`\x1b\[[0-9;]*[a-zA-Z]`, 'g'), '');
		let retryCmd = '';
		if (plain.includes('No conversation found with session ID:')) {
			// --resume failed → start a fresh session instead
			retryCmd = 'claude';
		}
		if (retryCmd) {
			claudeRetryCmd = retryCmd;
			earlyOutput = '';
			// The shell is still alive — just type the corrected command into it
			setTimeout(() => {
				writeTerminal(sessionId, `${retryCmd}\n`);
			}, 500);
		}
	}

	function debouncedFit() {
		if (resizeTimeout) clearTimeout(resizeTimeout);
		resizeTimeout = setTimeout(() => {
			if (fitAddon && terminal) {
				fitAddon.fit();
			}
		}, 50);
	}

	$effect(() => {
		if (active && fitAddon && terminal) {
			requestAnimationFrame(() => {
				fitAddon!.fit();
			});
		}
	});

	onMount(async () => {
		try {
			// Wait for fonts to load so cell measurements are accurate
			await document.fonts.ready;

			terminal = new Terminal(terminalOptions);
			fitAddon = new FitAddon();
			terminal.loadAddon(fitAddon);
			terminal.open(container);

			// Sync PTY size whenever xterm resizes (from FitAddon or any source)
			terminal.onResize(({ cols, rows }: { cols: number; rows: number }) => {
				void resizeTerminal(sessionId, cols, rows);
			});

			terminal.onData((data: string) => {
				writeTerminal(sessionId, data);
			});

			unlistenData = await onTerminalData((event) => {
				if (event.sessionId === sessionId && terminal) {
					terminal.write(event.data);
					detectClaudeRetry(event.data);
				}
			});

			unlistenExit = await onTerminalExit((event) => {
				if (event.sessionId === sessionId && terminal) {
					exited = true;
					terminal.writeln(`\r\n[process exited: ${event.exitCode}]`);
				}
			});

			// Fit before creating PTY so it starts with the correct size
			fitAddon.fit();

			await createTerminal({
				id: sessionId,
				projectPath: project.path,
				shell: project.shell || '',
				cols: terminal.cols,
				rows: terminal.rows,
				startupCommand
			});

			// Debounced resize on container changes to avoid feedback loops
			resizeObserver = new ResizeObserver(() => {
				debouncedFit();
			});
			resizeObserver.observe(container);
		} catch (error) {
			terminalError = `Failed to start terminal: ${String(error)}`;
		}
	});

	onDestroy(() => {
		if (resizeTimeout) clearTimeout(resizeTimeout);
		unlistenData?.();
		unlistenExit?.();
		resizeObserver?.disconnect();
		terminal?.dispose();
		if (!exited) {
			void killTerminal(sessionId);
		}
	});
</script>

<div class="terminal-wrapper">
	<div class="terminal-shell" bind:this={container}></div>
	{#if terminalError}
		<div class="terminal-error">{terminalError}</div>
	{/if}
</div>

<style>
	.terminal-wrapper {
		height: 100%;
		width: 100%;
		padding: 6px 8px;
		background: #1a1a1e;
		overflow: hidden;
		box-sizing: border-box;
	}

	.terminal-shell {
		height: 100%;
		width: 100%;
	}

	.terminal-error {
		position: absolute;
		inset: 0;
		display: flex;
		align-items: center;
		justify-content: center;
		color: #fca5a5;
		font-size: 13px;
		background: #1a1a1e;
	}
</style>
