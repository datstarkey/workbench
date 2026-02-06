<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import { Terminal } from 'xterm';
	import { FitAddon } from '@xterm/addon-fit';
	import 'xterm/css/xterm.css';
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

	// Buffer early output to detect Claude CLI errors for auto-retry
	let earlyOutput = '';
	let claudeRetryCmd = '';

	function detectClaudeRetry(data: string): void {
		if (!startupCommand?.startsWith('claude ') || claudeRetryCmd) return;
		earlyOutput += data;
		// Stop buffering after ~2KB
		if (earlyOutput.length > 2048) {
			earlyOutput = '';
			return;
		}
		const plain = earlyOutput.replace(new RegExp(String.raw`\x1b\[[0-9;]*[a-zA-Z]`, 'g'), '');
		let retryCmd = '';
		if (plain.includes('No conversation found with session ID:')) {
			// --resume failed → restart as new session
			const match = startupCommand.match(/claude --resume (\S+)/);
			if (match) retryCmd = `claude --session-id ${match[1]}`;
		} else if (
			plain.includes('already exists') ||
			(plain.includes('Session ID') && plain.includes('is already in use'))
		) {
			// --session-id failed → restart as resume
			const match = startupCommand.match(/claude --session-id (\S+)/);
			if (match) retryCmd = `claude --resume ${match[1]}`;
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

	async function syncResize() {
		if (!fitAddon || !terminal) return;
		fitAddon.fit();
		await resizeTerminal(sessionId, terminal.cols, terminal.rows);
	}

	$effect(() => {
		if (active && fitAddon && terminal) {
			requestAnimationFrame(() => {
				void syncResize();
			});
		}
	});

	onMount(async () => {
		try {
			terminal = new Terminal(terminalOptions);

			fitAddon = new FitAddon();
			terminal.loadAddon(fitAddon);
			terminal.open(container);

			terminal.onData((data) => {
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

			await createTerminal({
				id: sessionId,
				projectPath: project.path,
				shell: project.shell || '',
				cols: 120,
				rows: 30,
				startupCommand
			});

			await syncResize();

			resizeObserver = new ResizeObserver(() => {
				void syncResize();
			});
			resizeObserver.observe(container);
		} catch (error) {
			terminalError = `Failed to start terminal: ${String(error)}`;
		}
	});

	onDestroy(() => {
		unlistenData?.();
		unlistenExit?.();
		resizeObserver?.disconnect();
		terminal?.dispose();
		if (!exited) {
			void killTerminal(sessionId);
		}
	});
</script>

<div class="terminal-shell" bind:this={container}></div>
{#if terminalError}
	<div class="terminal-error">{terminalError}</div>
{/if}

<style>
	.terminal-shell {
		height: 100%;
		width: 100%;
		background: #1a1a1e;
		overflow: hidden;
	}

	.terminal-shell :global(.xterm) {
		padding: 6px 8px;
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
