<script lang="ts">
	import CircleAlertIcon from '@lucide/svelte/icons/circle-alert';
	import CirclePauseIcon from '@lucide/svelte/icons/circle-pause';
	import LoaderCircleIcon from '@lucide/svelte/icons/loader-circle';
	import RotateCcwIcon from '@lucide/svelte/icons/rotate-ccw';
	import XIcon from '@lucide/svelte/icons/x';
	import * as ContextMenu from '$lib/components/ui/context-menu';
	import type { ActiveClaudeSession } from '$types/workbench';

	let {
		session,
		onSelect,
		onRestart,
		onClose
	}: {
		session: ActiveClaudeSession;
		onSelect: () => void;
		onRestart: () => void;
		onClose: () => void;
	} = $props();

	const labelClass = $derived(
		session.awaitingInput
			? 'text-red-300'
			: session.needsAttention
				? session.sessionType === 'codex'
					? 'text-sky-300'
					: 'text-amber-300'
				: ''
	);
</script>

<ContextMenu.Root>
	<ContextMenu.Trigger class="w-full">
		<button
			class="flex w-full items-center gap-2 rounded-md px-2 py-1 text-left text-muted-foreground transition-colors hover:bg-accent/50 hover:text-foreground"
			type="button"
			onclick={onSelect}
		>
			{#if session.awaitingInput}
				<CircleAlertIcon class="size-3 shrink-0 text-red-400" />
			{:else if session.needsAttention}
				<CirclePauseIcon
					class={`size-3 shrink-0 ${session.sessionType === 'codex' ? 'text-sky-400' : 'text-amber-400'}`}
				/>
			{:else}
				<LoaderCircleIcon
					class={`size-3 shrink-0 animate-spin ${session.sessionType === 'codex' ? 'text-sky-400' : 'text-amber-400'}`}
				/>
			{/if}
			<span class={`truncate text-xs font-medium ${labelClass}`}>{session.label}</span>
		</button>
	</ContextMenu.Trigger>
	<ContextMenu.Content class="w-40">
		<ContextMenu.Item onclick={onRestart}>
			<RotateCcwIcon class="size-3.5" />
			Restart
		</ContextMenu.Item>
		<ContextMenu.Separator />
		<ContextMenu.Item class="text-destructive" onclick={onClose}>
			<XIcon class="size-3.5" />
			Close
		</ContextMenu.Item>
	</ContextMenu.Content>
</ContextMenu.Root>
