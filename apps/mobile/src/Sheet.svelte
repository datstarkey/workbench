<script lang="ts">
	import type { Snippet } from 'svelte';
	import { useBack } from './back-navigation';

	/** A bottom sheet over the current screen; `onClose` makes the backdrop and Esc dismiss it. */
	let {
		label,
		onClose,
		children
	}: {
		label: string;
		onClose?: () => void;
		children: Snippet;
	} = $props();
	useBack(() => onClose?.(), 1);
</script>

<svelte:window onkeydown={(e) => e.key === 'Escape' && onClose?.()} />

<div class="fixed inset-0 z-40 flex flex-col justify-end">
	<button
		type="button"
		class="scrim absolute inset-0 bg-black/60"
		aria-label="Close"
		tabindex="-1"
		disabled={!onClose}
		onclick={onClose}
	></button>
	<div
		role="dialog"
		aria-modal="true"
		aria-label={label}
		class="panel relative flex max-h-[85%] flex-col rounded-t-2xl border-t border-wb-hair bg-wb-panel shadow-2xl"
		style="padding-bottom: env(safe-area-inset-bottom)"
	>
		<span class="mx-auto mt-2 mb-2 h-1 w-9 shrink-0 rounded-full bg-wb-hair"></span>
		<div class="min-h-0 overflow-y-auto px-4 pb-4">
			{@render children()}
		</div>
	</div>
</div>

<style>
	.scrim {
		animation: fade 160ms ease-out;
	}
	.panel {
		animation: rise 220ms cubic-bezier(0.2, 0.8, 0.2, 1);
	}
	@keyframes fade {
		from {
			opacity: 0;
		}
	}
	@keyframes rise {
		from {
			transform: translateY(24px);
			opacity: 0;
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.scrim,
		.panel {
			animation: none;
		}
	}
</style>
