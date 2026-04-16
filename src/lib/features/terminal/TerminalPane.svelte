<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import { Terminal } from '@xterm/xterm';
	import { FitAddon } from '@xterm/addon-fit';
	import { WebLinksAddon } from '@xterm/addon-web-links';
	import { WebglAddon } from '@xterm/addon-webgl';
	import { LigaturesAddon } from '@xterm/addon-ligatures';
	import { SearchAddon } from '@xterm/addon-search';
	import { open } from '@tauri-apps/plugin-shell';
	import '@xterm/xterm/css/xterm.css';
	import type { ProjectConfig } from '$types/workbench';
	import { terminalOptions, TERMINAL_BG } from '$lib/terminal-config';
	import {
		createTerminal,
		writeTerminal,
		resizeTerminal,
		killTerminal,
		cleanupSessionInput,
		onSessionTerminalData,
		onSessionTerminalExit
	} from '$lib/utils/terminal';
	import { stripAnsi } from '$lib/utils/format';
	import TerminalSearch from './TerminalSearch.svelte';
	import { registerShellIntegration, type ShellIntegrationState } from './shell-integration';
	import { isLayoutDisabled } from './layout-guard';
	import { getClaudeSessionStore, getWorkbenchSettingsStore } from '$stores/context';

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

	// VS Code pattern: if WebGL fails once, all future terminals skip it
	let webglUnavailable = false;

	let container: HTMLDivElement;
	let terminal: Terminal | null = null;
	let fitAddon: FitAddon | null = null;
	let webglAddon: WebglAddon | null = null;
	let webLinksLoaded = false;
	let unlistenData: (() => void) | null = null;
	let unlistenExit: (() => void) | null = null;
	let resizeObserver: ResizeObserver | null = null;
	let resizeRAFId: number | null = null;
	let intersectionObserver: IntersectionObserver | null = null;
	let exited = false;
	let searchAddon = $state<SearchAddon | null>(null);
	let searchOpen = $state(false);
	let shellState: ShellIntegrationState | null = null;
	let terminalError = $state('');
	let removeVisibilityListener: (() => void) | null = null;
	let lastCols = 0;
	let lastRows = 0;
	let documentVisible = $state(document.visibilityState === 'visible');
	let terminalInView = $state(true);
	let perfLogInterval: ReturnType<typeof setInterval> | null = null;
	const claudeSessionStore = getClaudeSessionStore();
	const workbenchSettingsStore = getWorkbenchSettingsStore();

	// VS Code-style split-axis resize debouncing:
	// Rows resize immediately (cheap — just add/remove viewport lines).
	// Columns debounce at 100ms (expensive — every line must reflow).
	// Short buffers (<200 lines) resize both axes immediately.
	const COL_RESIZE_DEBOUNCE_MS = 100;
	const SHORT_BUFFER_THRESHOLD = 200;
	let colResizeTimeout: ReturnType<typeof setTimeout> | null = null;
	let pendingCols = 0;

	const OFFSCREEN_FLUSH_INTERVAL_MS = 80;
	const PERF_LOG_INTERVAL_MS = 10000;
	const SCROLLBACK_NORMAL = terminalOptions.scrollback ?? 5000;
	const SCROLLBACK_PERFORMANCE = 2000;

	let outputEventsSinceLog = 0;
	let outputBytesSinceLog = 0;
	let outputFlushesSinceLog = 0;
	let outputFlushMsSinceLog = 0;
	let inputEventsSinceLog = 0;
	let inputLatencySamplesSinceLog = 0;
	let inputLatencyTotalMsSinceLog = 0;
	let pendingInputAtMs: number | null = null;

	// Offscreen terminals still need periodic flushing
	let outputFlushTimer: ReturnType<typeof setTimeout> | null = null;
	let offscreenQueue = '';

	// Buffer early output to detect Claude CLI errors for auto-retry
	let earlyOutput = '';
	let claudeRetryCmd = '';

	function detectClaudeRetry(data: string): void {
		if (!startupCommand?.startsWith('claude') || claudeRetryCmd) return;
		earlyOutput += data;
		if (earlyOutput.length > 2048) {
			earlyOutput = '';
			return;
		}
		const plain = stripAnsi(earlyOutput);
		let retryCmd = '';
		if (plain.includes('No conversation found with session ID:')) {
			retryCmd = 'claude';
		}
		if (retryCmd) {
			claudeRetryCmd = retryCmd;
			earlyOutput = '';
			setTimeout(() => {
				writeTerminal(sessionId, `${retryCmd}\n`);
			}, 500);
		}
	}

	function canFitTerminal(): boolean {
		if (!terminal || !fitAddon || !container) return false;
		if (!active || !terminalInView) return false;
		if (!documentVisible) return false;
		const { width, height } = container.getBoundingClientRect();
		return width > 0 && height > 0;
	}

	function fitTerminal() {
		if (!canFitTerminal()) return;
		fitAddon!.fit();
	}

	// VS Code-style split-axis resize: fit the terminal, then apply
	// row changes immediately and column changes with a short debounce.
	function smartResize() {
		if (isLayoutDisabled()) return;
		if (!canFitTerminal() || !terminal || !fitAddon) return;

		// Use FitAddon to propose new dimensions without applying them
		const dims = fitAddon.proposeDimensions();
		if (!dims || dims.cols <= 0 || dims.rows <= 0) return;

		const colsChanged = dims.cols !== terminal.cols;
		const rowsChanged = dims.rows !== terminal.rows;
		if (!colsChanged && !rowsChanged) return;

		const isShortBuffer = terminal.buffer.normal.length < SHORT_BUFFER_THRESHOLD;

		if (isShortBuffer) {
			// Short buffers: resize both axes immediately (like VS Code)
			terminal.resize(dims.cols, dims.rows);
			return;
		}

		// Rows are cheap — apply immediately
		if (rowsChanged && !colsChanged) {
			terminal.resize(terminal.cols, dims.rows);
			return;
		}

		// Columns are expensive — debounce
		if (colsChanged) {
			// Apply row change immediately if there is one
			if (rowsChanged) {
				terminal.resize(terminal.cols, dims.rows);
			}
			// Debounce column change
			pendingCols = dims.cols;
			if (colResizeTimeout) clearTimeout(colResizeTimeout);
			colResizeTimeout = setTimeout(() => {
				if (!terminal) return;
				terminal.resize(pendingCols, terminal.rows);
				colResizeTimeout = null;
			}, COL_RESIZE_DEBOUNCE_MS);
		}
	}

	// Flush any pending column resize immediately (used on tab switch)
	function flushPendingResize() {
		if (colResizeTimeout && terminal && pendingCols > 0) {
			clearTimeout(colResizeTimeout);
			colResizeTimeout = null;
			terminal.resize(pendingCols, terminal.rows);
		}
	}

	function clearFlushSchedule() {
		if (outputFlushTimer) {
			clearTimeout(outputFlushTimer);
			outputFlushTimer = null;
		}
	}

	function inPerformanceMode(): boolean {
		if (!active || !documentVisible || !terminalInView) return true;
		return workbenchSettingsStore.terminalPerformanceMode === 'always';
	}

	function ensureWebLinksAddon() {
		if (!terminal || webLinksLoaded) return;
		terminal.loadAddon(new WebLinksAddon((_event, uri) => open(uri)));
		webLinksLoaded = true;
	}

	function loadWebGlAddon() {
		if (!terminal || webglUnavailable || webglAddon) return;
		try {
			webglAddon = new WebglAddon();
			webglAddon.onContextLoss(() => {
				webglAddon?.dispose();
				webglAddon = null;
				// After context loss, re-fit to recalculate cell metrics
				// (different renderers may have slightly different measurements)
				requestAnimationFrame(() => fitTerminal());
			});
			terminal.loadAddon(webglAddon);
		} catch {
			webglUnavailable = true;
			webglAddon = null;
		}
	}

	function logPerfSnapshotIfEnabled() {
		if (!workbenchSettingsStore.terminalTelemetryEnabled) return;
		const avgFlushMs =
			outputFlushesSinceLog > 0 ? outputFlushMsSinceLog / outputFlushesSinceLog : 0;
		const avgInputLatencyMs =
			inputLatencySamplesSinceLog > 0
				? inputLatencyTotalMsSinceLog / inputLatencySamplesSinceLog
				: 0;

		console.info('[TerminalPerf]', {
			sessionId,
			performanceMode: inPerformanceMode(),
			outputEvents: outputEventsSinceLog,
			outputBytes: outputBytesSinceLog,
			outputFlushes: outputFlushesSinceLog,
			avgFlushMs: Number(avgFlushMs.toFixed(2)),
			inputEvents: inputEventsSinceLog,
			avgInputLatencyMs: Number(avgInputLatencyMs.toFixed(2))
		});

		outputEventsSinceLog = 0;
		outputBytesSinceLog = 0;
		outputFlushesSinceLog = 0;
		outputFlushMsSinceLog = 0;
		inputEventsSinceLog = 0;
		inputLatencySamplesSinceLog = 0;
		inputLatencyTotalMsSinceLog = 0;
	}

	// VS Code pattern: write data directly to xterm with a callback.
	// Active terminals get immediate writes. Offscreen terminals batch
	// into a queue flushed on a timer to avoid wasted rendering work.
	function writeTerminalData(data: string) {
		outputEventsSinceLog += 1;
		outputBytesSinceLog += data.length;

		if (pendingInputAtMs !== null && data.length > 0) {
			inputLatencySamplesSinceLog += 1;
			inputLatencyTotalMsSinceLog += performance.now() - pendingInputAtMs;
			pendingInputAtMs = null;
		}

		detectClaudeRetry(data);

		if (inPerformanceMode()) {
			// Offscreen: batch into queue, flush on timer
			offscreenQueue += data;
			if (!outputFlushTimer) {
				outputFlushTimer = setTimeout(() => {
					outputFlushTimer = null;
					if (!terminal || offscreenQueue.length === 0) return;
					const batched = offscreenQueue;
					offscreenQueue = '';
					const start = performance.now();
					terminal.write(batched);
					outputFlushesSinceLog += 1;
					outputFlushMsSinceLog += performance.now() - start;
				}, OFFSCREEN_FLUSH_INTERVAL_MS);
			}
			return;
		}

		// Active terminal: write directly with callback (VS Code pattern)
		const start = performance.now();
		terminal?.write(data, () => {
			outputFlushesSinceLog += 1;
			outputFlushMsSinceLog += performance.now() - start;
		});
	}

	// On tab switch: flush pending resizes synchronously, fit terminal,
	// and flush any buffered offscreen data
	$effect(() => {
		if (active && fitAddon && terminal) {
			flushPendingResize();
			// With overlay model, the container maintains dimensions when hidden
			// (visibility:hidden instead of display:none). Only re-fit if
			// dimensions actually changed (e.g., window was resized while this
			// tab was inactive).
			const dims = fitAddon.proposeDimensions();
			if (dims && (dims.cols !== terminal.cols || dims.rows !== terminal.rows)) {
				fitTerminal();
			}
			// Flush offscreen buffer if there's data waiting
			if (offscreenQueue.length > 0) {
				const batched = offscreenQueue;
				offscreenQueue = '';
				terminal.write(batched);
			}
		}
	});

	$effect(() => {
		void active;
		void documentVisible;
		void terminalInView;
		void workbenchSettingsStore.terminalPerformanceMode;
		const performanceMode = inPerformanceMode();
		if (terminal) {
			terminal.options.scrollback = performanceMode ? SCROLLBACK_PERFORMANCE : SCROLLBACK_NORMAL;
			if (!performanceMode) ensureWebLinksAddon();
		}
		clearFlushSchedule();
	});

	onMount(async () => {
		try {
			// Wait for fonts to load so cell measurements are accurate
			await document.fonts.ready;

			terminal = new Terminal({
				...terminalOptions,
				scrollback: inPerformanceMode() ? SCROLLBACK_PERFORMANCE : SCROLLBACK_NORMAL
			});
			fitAddon = new FitAddon();
			terminal.loadAddon(fitAddon);
			searchAddon = new SearchAddon({ highlightLimit: 1000 });
			terminal.loadAddon(searchAddon);

			if (!inPerformanceMode()) {
				ensureWebLinksAddon();
			}
			// Intercept a key event: tell xterm.js to skip its own handling AND
			// prevent the browser from firing the follow-up keypress/input event
			// on xterm's hidden textarea (which would otherwise re-emit the key
			// via onData — the root cause of Shift+Enter duplication).
			// This matches VS Code's xterm integration pattern.
			const intercept = (event: KeyboardEvent): false => {
				event.preventDefault();
				event.stopPropagation();
				return false;
			};

			terminal.attachCustomKeyEventHandler((event: KeyboardEvent) => {
				// Ctrl+F / Cmd+F -> open search
				if (event.key === 'f' && (event.ctrlKey || event.metaKey) && !event.shiftKey) {
					if (event.type === 'keydown') {
						searchOpen = true;
					}
					return intercept(event);
				}

				// Escape with search open -> close search instead of forwarding to PTY
				if (event.key === 'Escape' && searchOpen) {
					if (event.type === 'keydown') {
						searchOpen = false;
						terminal?.focus();
					}
					return intercept(event);
				}

				// Always forward Escape directly to PTY. xterm.js may swallow it
				// (e.g. to clear a selection) which makes it feel unresponsive in TUIs.
				if (event.key === 'Escape') {
					if (event.type === 'keydown') {
						const isAI = claudeSessionStore.paneType(sessionId) !== null;
						writeTerminal(sessionId, isAI ? '\x1b\x1b' : '\x1b');
					}
					return intercept(event);
				}
				// Always forward Ctrl+C as interrupt.
				if (event.key === 'c' && event.ctrlKey && !event.shiftKey && !event.metaKey) {
					if (event.type === 'keydown') {
						writeTerminal(sessionId, '\x03');
					}
					return intercept(event);
				}
				if (
					event.key === 'Enter' &&
					event.shiftKey &&
					!event.altKey &&
					!event.ctrlKey &&
					!event.metaKey
				) {
					if (event.type === 'keydown') {
						if (claudeSessionStore.paneType(sessionId) === 'codex') {
							writeTerminal(sessionId, '\x0A');
						} else {
							writeTerminal(sessionId, '\x1b[200~\n\x1b[201~');
						}
					}
					return intercept(event);
				}
				// Ctrl+Shift+Up/Down -> navigate between prompt boundaries
				if (
					event.ctrlKey &&
					event.shiftKey &&
					(event.key === 'ArrowUp' || event.key === 'ArrowDown')
				) {
					if (event.type === 'keydown' && shellState && terminal) {
						const buf = terminal.buffer.active;
						const currentLine = buf.viewportY;
						const target =
							event.key === 'ArrowUp'
								? shellState.previousPromptLine(currentLine)
								: shellState.nextPromptLine(currentLine);
						if (target !== null) {
							terminal.scrollToLine(target);
						}
					}
					return intercept(event);
				}
				return true;
			});

			// VS Code: open terminal first, THEN load WebGL addon
			// (WebGL needs the canvas element to exist)
			terminal.open(container);

			// Load addons that require the canvas element to exist
			loadWebGlAddon();
			try {
				terminal.loadAddon(new LigaturesAddon());
			} catch (error) {
				console.warn('[TerminalPane] Ligatures addon failed to load:', error);
			}

			// Shell integration: parse OSC 133 sequences for prompt/command tracking
			shellState = registerShellIntegration(terminal);

			// Sync PTY size whenever xterm resizes (from FitAddon or any source)
			terminal.onResize(({ cols, rows }: { cols: number; rows: number }) => {
				if (cols <= 0 || rows <= 0) return;
				lastCols = cols;
				lastRows = rows;
				claudeSessionStore.noteLocalViewportChange(sessionId);
				void resizeTerminal(sessionId, cols, rows);
			});

			terminal.onData((data: string) => {
				claudeSessionStore.noteLocalInput(sessionId, data);
				inputEventsSinceLog += 1;
				pendingInputAtMs = performance.now();
				writeTerminal(sessionId, data);
			});

			unlistenData = await onSessionTerminalData(sessionId, (event) => {
				writeTerminalData(event.data);
			});

			unlistenExit = await onSessionTerminalExit(sessionId, (event) => {
				exited = true;
				terminal?.writeln(`\r\n[process exited: ${event.exitCode}]`);
			});

			// Fit before creating PTY so it starts with the correct size.
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

			// VS Code-style resize: use ResizeObserver but with smart
			// split-axis debouncing instead of a flat 500ms delay
			resizeObserver = new ResizeObserver(() => {
				if (resizeRAFId !== null) return;
				resizeRAFId = requestAnimationFrame(() => {
					resizeRAFId = null;
					smartResize();
				});
			});
			resizeObserver.observe(container);
			intersectionObserver = new IntersectionObserver(
				(entries) => {
					const entry = entries[0];
					terminalInView = Boolean(entry?.isIntersecting && entry.intersectionRatio > 0);
					if (!active || !documentVisible || !terminalInView) return;
					claudeSessionStore.noteLocalViewportChange(sessionId);
					fitTerminal();
				},
				{ threshold: 0.01 }
			);
			intersectionObserver.observe(container);

			const onVisibilityChange = () => {
				documentVisible = document.visibilityState === 'visible';
				if (!active || !documentVisible || !terminalInView) return;
				claudeSessionStore.noteLocalViewportChange(sessionId);
				flushPendingResize();
				fitTerminal();
			};
			document.addEventListener('visibilitychange', onVisibilityChange);
			removeVisibilityListener = () => {
				document.removeEventListener('visibilitychange', onVisibilityChange);
			};
			if (workbenchSettingsStore.terminalTelemetryEnabled) {
				perfLogInterval = setInterval(logPerfSnapshotIfEnabled, PERF_LOG_INTERVAL_MS);
			}
		} catch (error) {
			terminalError = `Failed to start terminal: ${String(error)}`;
		}
	});

	onDestroy(() => {
		if (resizeRAFId !== null) cancelAnimationFrame(resizeRAFId);
		if (colResizeTimeout) clearTimeout(colResizeTimeout);
		clearFlushSchedule();
		if (perfLogInterval) clearInterval(perfLogInterval);
		removeVisibilityListener?.();
		unlistenData?.();
		unlistenExit?.();
		resizeObserver?.disconnect();
		intersectionObserver?.disconnect();
		cleanupSessionInput(sessionId);
		shellState?.dispose();
		searchAddon?.dispose();
		webglAddon?.dispose();
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
	{#if searchOpen && searchAddon}
		<TerminalSearch
			{searchAddon}
			onClose={() => {
				searchOpen = false;
				terminal?.focus();
			}}
		/>
	{/if}
</div>

<style>
	.terminal-wrapper {
		position: relative;
		height: 100%;
		width: 100%;
		padding: 6px 8px;
		overflow: hidden;
		box-sizing: border-box;
		contain: strict;
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
