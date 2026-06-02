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

	const isClaude = $derived(session.sessionType === 'claude');

	const labelClass = $derived(
		session.awaitingInput
			? 'text-wb-err'
			: session.needsAttention
				? isClaude
					? 'text-wb-warn'
					: 'text-wb-codex'
				: 'text-wb-ink-mute'
	);

	const badgeClass = $derived(
		isClaude ? 'bg-wb-claude/20 text-wb-claude' : 'bg-wb-codex/20 text-wb-codex'
	);

	const badgeLabel = $derived(isClaude ? 'CLA' : 'COD');
</script>

<ContextMenu.Root>
	<ContextMenu.Trigger class="w-full">
		<button
			class="flex w-full items-center gap-1.5 px-2 py-[4px] text-left transition-colors hover:bg-wb-panel2"
			type="button"
			onclick={onSelect}
		>
			{#if session.awaitingInput}
				<CircleAlertIcon class="size-3 shrink-0 text-wb-err" />
			{:else if session.needsAttention}
				<CirclePauseIcon class={['size-3 shrink-0', isClaude ? 'text-wb-warn' : 'text-wb-codex']} />
			{:else}
				<LoaderCircleIcon
					class={['size-3 shrink-0 animate-spin', isClaude ? 'text-wb-warn' : 'text-wb-codex']}
				/>
			{/if}
			<span class={['truncate font-mono text-[11px]', labelClass]}>{session.label}</span>
			<span
				class={['ml-auto shrink-0 rounded px-1 font-mono text-[9.5px] font-semibold', badgeClass]}
				>{badgeLabel}</span
			>
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
