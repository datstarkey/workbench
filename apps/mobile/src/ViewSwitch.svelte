<script lang="ts">
	import MessageSquareIcon from '@lucide/svelte/icons/message-square';
	import SquareTerminalIcon from '@lucide/svelte/icons/square-terminal';
	import { cn } from '@workbench/ui';

	/** Chat | Terminal for one Claude conversation; tapping the other side switches it. */
	let {
		view,
		disabled = false,
		onSwitch
	}: {
		view: 'chat' | 'terminal';
		disabled?: boolean;
		onSwitch: () => void;
	} = $props();

	const sides = [
		{ id: 'chat', label: 'Chat', Icon: MessageSquareIcon },
		{ id: 'terminal', label: 'Terminal', Icon: SquareTerminalIcon }
	] as const;
</script>

<div
	class="flex shrink-0 rounded-[9px] border border-wb-hair bg-wb-panel2 p-0.5"
	role="group"
	aria-label="View"
>
	{#each sides as side (side.id)}
		<button
			type="button"
			class={cn(
				'grid h-7 w-9 place-items-center rounded-[7px] transition-colors disabled:opacity-50',
				view === side.id ? 'bg-wb-bg text-wb-ink shadow-sm' : 'text-wb-ink-soft'
			)}
			aria-label={side.label}
			aria-pressed={view === side.id}
			disabled={disabled || view === side.id}
			onclick={onSwitch}
		>
			<side.Icon class="size-4" />
		</button>
	{/each}
</div>
