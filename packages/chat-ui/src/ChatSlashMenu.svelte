<script lang="ts">
	import type { Attachment } from 'svelte/attachments';
	import { watch } from 'runed';
	import { cn } from '@workbench/ui';
	import type { SlashCommand } from '@workbench/types';

	let {
		id,
		commands,
		active,
		onPick,
		onHover
	}: {
		id: string;
		/** Already filtered and ranked. */
		commands: SlashCommand[];
		active: number;
		onPick: (command: SlashCommand) => void;
		onHover: (index: number) => void;
	} = $props();

	/** Keep the highlighted row in view while arrowing through a long list. */
	const followActive: Attachment<HTMLUListElement> = (node) => {
		watch(
			() => active,
			(i) => {
				node.children[i]?.scrollIntoView({ block: 'nearest' });
			}
		);
	};
</script>

<ul
	{id}
	{@attach followActive}
	role="listbox"
	aria-label="Commands"
	class="scrollbar-thin absolute inset-x-0 bottom-full z-30 mb-2 max-h-72 overflow-y-auto rounded-lg border border-wb-hair bg-wb-panel p-1 shadow-xl"
>
	{#each commands as command, i (command.name)}
		<li
			id="{id}-{i}"
			role="option"
			aria-selected={i === active}
			class={cn(
				'flex cursor-pointer items-baseline gap-2 rounded-md px-2.5 py-1.5 text-xs',
				i === active ? 'bg-wb-accent-soft' : 'hover:bg-wb-panel2'
			)}
			onmousedown={(e) => {
				e.preventDefault(); // keep focus in the composer
				onPick(command);
			}}
			onmouseenter={() => onHover(i)}
		>
			<span class="shrink-0 font-mono text-wb-ink">/{command.name}</span>
			{#if command.argumentHint}
				<span class="shrink-0 font-mono text-[11px] text-wb-ink-soft">{command.argumentHint}</span>
			{/if}
			<span class="min-w-0 truncate text-wb-ink-mute">{command.description}</span>
		</li>
	{/each}
</ul>
