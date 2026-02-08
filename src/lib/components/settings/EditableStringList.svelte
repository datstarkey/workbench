<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Badge } from '$lib/components/ui/badge';
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

<div class="mt-2 space-y-1.5">
	{#each items as item, i (item + i)}
		<div class="flex items-center gap-1.5">
			<Badge variant={badgeVariant} class="flex-1 justify-start font-mono text-xs">
				{item}
			</Badge>
			<Button
				type="button"
				variant="ghost"
				size="icon-sm"
				class="size-6 shrink-0 text-muted-foreground hover:text-destructive"
				onclick={() => onRemove(item)}
			>
				<XIcon class="size-3" />
			</Button>
		</div>
	{/each}
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
		<Input class="h-7 flex-1 font-mono text-xs" {placeholder} bind:value={newValue} />
		<Button variant="outline" size="icon-sm" class="size-7 shrink-0" type="submit">
			<PlusIcon class="size-3" />
		</Button>
	</form>
</div>
