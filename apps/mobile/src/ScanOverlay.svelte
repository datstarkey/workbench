<script lang="ts">
	import type { Attachment } from 'svelte/attachments';

	let { onCancel }: { onCancel: () => void } = $props();

	// The windowed scanner draws the camera behind the webview, so the page must
	// be see-through while this overlay is up.
	const transparentPage: Attachment = () => {
		document.documentElement.classList.add('qr-scanning');
		return () => document.documentElement.classList.remove('qr-scanning');
	};
</script>

<div
	{@attach transparentPage}
	class="flex h-full flex-col items-center justify-between px-6 text-white"
	style="padding-top: calc(env(safe-area-inset-top) + 2rem); padding-bottom: calc(env(safe-area-inset-bottom) + 2rem);"
>
	<p class="relative z-10 rounded-full bg-black/60 px-4 py-2 text-center text-sm">
		Point the camera at the QR code in Workbench on your computer
	</p>

	<div
		class="aspect-square w-full max-w-72 rounded-2xl border-2 border-white/90 shadow-[0_0_0_100vmax_rgba(0,0,0,0.45)]"
	></div>

	<div class="relative z-10 flex w-full max-w-72 flex-col items-center gap-2">
		<button
			class="w-full rounded-xl bg-white py-3 text-base font-semibold text-black active:bg-white/80"
			onclick={onCancel}
		>
			Cancel
		</button>
		<p class="text-center text-[11px] text-white/80">
			No camera picture? Cancel and enter the server details manually.
		</p>
	</div>
</div>
