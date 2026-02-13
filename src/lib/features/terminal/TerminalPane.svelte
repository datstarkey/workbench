<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import { Terminal } from '@xterm/xterm';
	import { FitAddon } from '@xterm/addon-fit';
	import { WebLinksAddon } from '@xterm/addon-web-links';
	import { open } from '@tauri-apps/plugin-shell';
	import '@xterm/xterm/css/xterm.css';
	import type { ProjectConfig } from '$types/workbench';
	import { terminalOptions, TERMINAL_BG } from '$lib/terminal-config';
	import {
		createTerminal,
		writeTerminal,
		resizeTerminal,
		killTerminal,
		signalForeground,
		onTerminalData,
		onTerminalExit
	} from '$lib/utils/terminal';
	import { stripAnsi } from '$lib/utils/format';
	import { getClaudeSessionStore } from '$stores/context';

	let {
		sessionId,
		project,
		active,
		startupCommand,
		cwd
	}: {
		sessionId: string;
		project: ProjectConfig;
		active: boolean;
		startupCommand?: string;
		cwd?: string;
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
	let removeVisibilityListener: (() => void) | null = null;
	let lastCols = 0;
	let lastRows = 0;
	const claudeSessionStore = getClaudeSessionStore();

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
		const plain = stripAnsi(earlyOutput);
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
			fitTerminal();
		}, 50);
	}

	function canFitTerminal(): boolean {
		if (!terminal || !fitAddon || !container) return false;
		if (document.visibilityState !== 'visible') return false;
		const { width, height } = container.getBoundingClientRect();
		return width > 0 && height > 0;
	}

	function fitTerminal() {
		if (!canFitTerminal()) return;
		fitAddon!.fit();
	}

	$effect(() => {
		if (active && fitAddon && terminal) {
			requestAnimationFrame(() => {
				fitTerminal();
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
			terminal.loadAddon(new WebLinksAddon((_event, uri) => open(uri)));
			let escFallbackTimer: ReturnType<typeof setTimeout> | null = null;

			terminal.attachCustomKeyEventHandler((event: KeyboardEvent) => {
				// Always forward Escape directly to PTY. xterm.js may swallow it
				// (e.g. to clear a selection) which makes it feel unresponsive in TUIs.
				if (event.key === 'Escape') {
					if (event.type === 'keydown') {
						if (escFallbackTimer) clearTimeout(escFallbackTimer);

						// For Claude/Codex TUIs, send \x1b\x1b so the input library
						// can instantly disambiguate standalone Escape from a CSI
						// sequence start, bypassing the ~100ms detection timeout.
						const isAI = claudeSessionStore.paneType(sessionId) !== null;
						writeTerminal(sessionId, isAI ? '\x1b\x1b' : '\x1b');

						// Fallback: if still generating after 400ms, escalate to
						// SIGINT delivered directly to the foreground process.
						if (isAI && claudeSessionStore.panesInProgress.has(sessionId)) {
							escFallbackTimer = setTimeout(() => {
								if (claudeSessionStore.panesInProgress.has(sessionId)) {
									signalForeground(sessionId);
								}
							}, 400);
						}
					}
					return false;
				}
				// Always forward Ctrl+C as interrupt. xterm.js copies to clipboard
				// when text is selected; on macOS Cmd+C handles copy instead.
				if (event.key === 'c' && event.ctrlKey && !event.shiftKey && !event.metaKey) {
					if (event.type === 'keydown') {
						writeTerminal(sessionId, '\x03');
					}
					return false;
				}
				if (
					event.key === 'Enter' &&
					event.shiftKey &&
					!event.altKey &&
					!event.ctrlKey &&
					!event.metaKey
				) {
					// Block both keydown and keypress so xterm never sends \r.
					// Only send the newline on keydown to avoid double-firing.
					if (event.type === 'keydown') {
						// Write directly to PTY, bypassing xterm's paste pipeline.
						// Codex: Ctrl+J (ASCII LF). Claude: bracketed paste newline.
						if (claudeSessionStore.paneType(sessionId) === 'codex') {
							writeTerminal(sessionId, '\x0A');
						} else {
							writeTerminal(sessionId, '\x1b[200~\n\x1b[201~');
						}
					}
					return false;
				}
				return true;
			});
			terminal.open(container);

			// Sync PTY size whenever xterm resizes (from FitAddon or any source)
			terminal.onResize(({ cols, rows }: { cols: number; rows: number }) => {
				// Ignore hidden/invalid fits that can transiently report 0x0 while backgrounded.
				if (cols <= 0 || rows <= 0) return;
				lastCols = cols;
				lastRows = rows;
				claudeSessionStore.noteLocalViewportChange(sessionId);
				void resizeTerminal(sessionId, cols, rows);
			});

			terminal.onData((data: string) => {
				claudeSessionStore.noteLocalInput(sessionId, data);
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

			// Fit before creating PTY so it starts with the correct size.
			// If the view is hidden/backgrounded, keep a sane fallback instead of 0x0.
			fitTerminal();
			if (terminal.cols <= 0 || terminal.rows <= 0) {
				terminal.resize(lastCols > 0 ? lastCols : 80, lastRows > 0 ? lastRows : 24);
			}

			await createTerminal({
				id: sessionId,
				projectPath: cwd ?? project.path,
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

			const onVisibilityChange = () => {
				if (!active || document.visibilityState !== 'visible') return;
				claudeSessionStore.noteLocalViewportChange(sessionId);
				requestAnimationFrame(() => fitTerminal());
			};
			document.addEventListener('visibilitychange', onVisibilityChange);
			removeVisibilityListener = () => {
				document.removeEventListener('visibilitychange', onVisibilityChange);
			};
		} catch (error) {
			terminalError = `Failed to start terminal: ${String(error)}`;
		}
	});

	onDestroy(() => {
		if (resizeTimeout) clearTimeout(resizeTimeout);
		removeVisibilityListener?.();
		unlistenData?.();
		unlistenExit?.();
		resizeObserver?.disconnect();
		terminal?.dispose();
		if (!exited) {
			void killTerminal(sessionId);
		}
	});
</script>

<div class="terminal-wrapper" style:background={TERMINAL_BG}>
	<div class="terminal-shell" bind:this={container}></div>
	{#if terminalError}
		<div class="terminal-error" style:background={TERMINAL_BG}>
			<div class="text-center">
				<p>{terminalError}</p>
				<button
					class="mt-2 rounded bg-white/10 px-3 py-1 text-xs hover:bg-white/20"
					type="button"
					onclick={() => (terminalError = '')}
				>
					Dismiss
				</button>
			</div>
		</div>
	{/if}
</div>

<style>
	.terminal-wrapper {
		height: 100%;
		width: 100%;
		padding: 6px 8px;
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
	}
</style>
