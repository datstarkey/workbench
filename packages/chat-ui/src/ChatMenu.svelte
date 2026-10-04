<script lang="ts" generics="T">
	import type { Snippet } from 'svelte';
	import type { Attachment } from 'svelte/attachments';
	import { watch } from 'runed';
	import { cn } from '@workbench/ui';

	let {
		id,
		label,
		items,
		key,
		active,
		onPick,
		onHover,
		row
	}: {
		id: string;
		label: string;
		/** Already filtered and ranked. */
		items: T[];
		key: (item: T) => string;
		active: number;
		onPick: (item: T) => void;
		onHover: (index: number) => void;
		row: Snippet<[T]>;
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
	aria-label={label}
	class="scrollbar-thin absolute inset-x-0 bottom-full z-30 mb-2 max-h-72 overflow-y-auto rounded-lg border border-wb-hair bg-wb-panel p-1 shadow-xl"
>
	{#each items as item, i (key(item))}
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
				onPick(item);
			}}
			onmouseenter={() => onHover(i)}
		>
			{@render row(item)}
		</li>
	{/each}
</ul>
