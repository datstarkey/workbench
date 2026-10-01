<script lang="ts">
	import { Button } from '@workbench/ui/button';
	import { Input } from '@workbench/ui/input';
	import { Badge } from '@workbench/ui/badge';
	import XIcon from '@lucide/svelte/icons/x';
	import PlusIcon from '@lucide/svelte/icons/plus';

	let {
		items,
		onAdd,
		onRemove,
		placeholder,
		badgeVariant = 'secondary'
	}: {
		items: string[];
		onAdd: (value: string) => void;
		onRemove: (value: string) => void;
		placeholder: string;
		badgeVariant?: 'secondary' | 'destructive' | 'outline';
	} = $props();

	let newValue = $state('');
</script>

<div class="space-y-2">
	{#if items.length > 0}
		<div class="flex flex-wrap gap-1.5">
			{#each items as item, i (item + i)}
				<Badge variant={badgeVariant} class="h-6 gap-1 pr-0.5 font-mono text-[11.5px]">
					{item}
					<button
						type="button"
						class="inline-flex size-4.5 items-center justify-center rounded-sm opacity-70 hover:opacity-100"
						aria-label="Remove {item}"
						onclick={() => onRemove(item)}
					>
						<XIcon class="size-3" />
					</button>
				</Badge>
			{/each}
		</div>
	{/if}
	<form
		class="flex items-center gap-1.5"
		onsubmit={(e) => {
			e.preventDefault();
			if (newValue.trim()) {
				onAdd(newValue.trim());
				newValue = '';
			}
		}}
	>
		<Input
			class="h-7 flex-1 font-mono text-xs"
			{placeholder}
			aria-label={placeholder}
			bind:value={newValue}
		/>
		<Button variant="outline" size="sm" class="h-7 shrink-0 gap-1 text-xs" type="submit">
			<PlusIcon class="size-3" />
			Add
		</Button>
	</form>
</div>
