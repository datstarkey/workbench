<script lang="ts">
	import MinusIcon from '@lucide/svelte/icons/minus';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import UndoIcon from '@lucide/svelte/icons/undo-2';
	import { Button } from '$lib/components/ui/button';
	import type { GitFileStatus } from '$types/workbench';

	let {
		file,
		onAction,
		onDiscard,
		actionIcon
	}: {
		file: GitFileStatus;
		onAction: () => void;
		onDiscard?: () => void;
		actionIcon: 'plus' | 'minus';
	} = $props();

	const statusDisplay: Record<string, { letter: string; color: string }> = {
		modified: { letter: 'M', color: 'text-yellow-400' },
		added: { letter: 'A', color: 'text-green-400' },
		deleted: { letter: 'D', color: 'text-red-400' },
		renamed: { letter: 'R', color: 'text-blue-400' },
		copied: { letter: 'C', color: 'text-blue-400' },
		untracked: { letter: '?', color: 'text-muted-foreground' }
	};

	let display = $derived(
		statusDisplay[file.status] ?? { letter: '?', color: 'text-muted-foreground' }
	);
</script>

<div class="group flex items-center gap-1.5 rounded px-1.5 py-0.5 hover:bg-muted/40">
	<span class="w-3 shrink-0 text-center font-mono text-xs font-medium {display.color}">
		{display.letter}
	</span>
	<span class="min-w-0 flex-1 truncate font-mono text-xs text-foreground/80" title={file.path}>
		{file.path}
	</span>
	<div class="flex shrink-0 items-center gap-0.5 opacity-0 group-hover:opacity-100">
		{#if onDiscard}
			<Button
				variant="ghost"
				size="icon-sm"
				class="size-5"
				onclick={onDiscard}
				title="Discard changes"
			>
				<UndoIcon class="size-3" />
			</Button>
		{/if}
		<Button
			variant="ghost"
			size="icon-sm"
			class="size-5"
			onclick={onAction}
			title={actionIcon === 'plus' ? 'Stage file' : 'Unstage file'}
		>
			{#if actionIcon === 'plus'}
				<PlusIcon class="size-3" />
			{:else}
				<MinusIcon class="size-3" />
			{/if}
		</Button>
	</div>
</div>
