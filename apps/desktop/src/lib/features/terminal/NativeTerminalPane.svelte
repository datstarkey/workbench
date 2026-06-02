<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import type { ProjectConfig } from '$types/workbench';
	import { TERMINAL_BG } from '$lib/terminal-config';
	import {
		createNativeTerminal,
		killNativeTerminal,
		resizeNativeTerminal,
		setNativeTerminalVisible,
		onSessionTerminalExit
	} from '$lib/utils/terminal';

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
	let unlistenExit: (() => void) | null = null;
	let resizeObserver: ResizeObserver | null = null;
	let mutationObserver: MutationObserver | null = null;
	let exited = $state(false);
	let terminalError = $state('');
	let resizeTimeout: ReturnType<typeof setTimeout> | null = null;
	let created = $state(false);
	let overlayOpen = $state(false);

	// Convert DOM coordinates to NSView coordinates.
	// NSView uses points (same as CSS pixels) but y=0 is at the bottom.
	function domToNSView(rect: DOMRect): { x: number; y: number; width: number; height: number } {
		const windowHeight = window.innerHeight;
		return {
			x: rect.left,
			y: windowHeight - rect.bottom, // NSView y is from bottom
			width: rect.width,
			height: rect.height
		};
	}

	// Detect open overlays (sheets, dialogs) that render in the webview
	// and would appear behind the native terminal view.
	function checkOverlayOpen(): boolean {
		return document.querySelector('[data-state="open"][role="dialog"]') !== null;
	}

	function debouncedResize() {
		if (resizeTimeout) clearTimeout(resizeTimeout);
		resizeTimeout = setTimeout(() => {
			if (!container || !created || exited) return;
			const rect = container.getBoundingClientRect();
			if (rect.width <= 0 || rect.height <= 0) return;
			const nsRect = domToNSView(rect);
			void resizeNativeTerminal(sessionId, nsRect.x, nsRect.y, nsRect.width, nsRect.height);
		}, 100);
	}

	// Show/hide based on active state AND overlay state.
	// Also re-measure and resize when becoming active, since the container
	// may have been display:none (class:hidden) when first mounted.
	$effect(() => {
		if (!created || exited) return;
		const shouldBeVisible = active && !overlayOpen;
		void setNativeTerminalVisible(sessionId, shouldBeVisible);
		if (active && container) {
			requestAnimationFrame(() => {
				const rect = container.getBoundingClientRect();
				if (rect.width > 0 && rect.height > 0) {
					const nsRect = domToNSView(rect);
					void resizeNativeTerminal(sessionId, nsRect.x, nsRect.y, nsRect.width, nsRect.height);
				}
			});
		}
	});

	onMount(async () => {
		try {
			const rect = container.getBoundingClientRect();
			const nsRect = domToNSView(rect);

			await createNativeTerminal({
				sessionId,
				projectPath: cwd ?? project.path,
				shell: project.shell || '',
				x: nsRect.x,
				y: nsRect.y,
				width: nsRect.width,
				height: nsRect.height,
				fontSize: 13,
				startupCommand
			});

			created = true;

			// Set initial visibility
			if (!active) {
				await setNativeTerminalVisible(sessionId, false);
			}

			unlistenExit = await onSessionTerminalExit(sessionId, () => {
				exited = true;
			});

			// Track size changes
			resizeObserver = new ResizeObserver(() => {
				debouncedResize();
			});
			resizeObserver.observe(container);

			// Watch for overlay dialogs/sheets so we can hide the native
			// terminal view (it renders above the webview).
			overlayOpen = checkOverlayOpen();
			mutationObserver = new MutationObserver(() => {
				overlayOpen = checkOverlayOpen();
			});
			mutationObserver.observe(document.body, {
				attributes: true,
				attributeFilter: ['data-state'],
				subtree: true,
				childList: true
			});
		} catch (error) {
			terminalError = `Failed to start native terminal: ${String(error)}`;
		}
	});

	onDestroy(() => {
		if (resizeTimeout) clearTimeout(resizeTimeout);
		unlistenExit?.();
		resizeObserver?.disconnect();
		mutationObserver?.disconnect();
		if (created && !exited) {
			void killNativeTerminal(sessionId);
		}
	});
</script>

<div class="native-terminal-wrapper" style:background={TERMINAL_BG} bind:this={container}>
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
	{#if exited}
		<div class="terminal-error" style:background={TERMINAL_BG}>
			<p class="text-sm text-muted-foreground">[process exited]</p>
		</div>
	{/if}
</div>

<style>
	.native-terminal-wrapper {
		position: relative;
		height: 100%;
		width: 100%;
		overflow: hidden;
		box-sizing: border-box;
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
