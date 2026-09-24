<script lang="ts">
	import MinusIcon from '@lucide/svelte/icons/minus';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import UndoIcon from '@lucide/svelte/icons/undo-2';
	import type { GitFileStatus } from '$types/workbench';
	import { iconButton } from '$features/sidebar/styles';
	import { getStatusDisplay, splitPath } from './git-view';

	let {
		file,
		staged,
		onToggle,
		onDiscard
	}: {
		file: GitFileStatus;
		staged: boolean;
		onToggle: () => void;
		onDiscard?: () => void;
	} = $props();

	let display = $derived(getStatusDisplay(file.status));
	let parts = $derived(splitPath(file.path));
</script>

<div
	class="group mx-1 flex h-6 items-center gap-2 rounded-[5px] pr-1.5 pl-[22px] hover:bg-wb-panel2"
	title={file.path}
>
	<span
		class={[
			'min-w-0 truncate text-xs',
			file.status === 'deleted' ? 'text-wb-ink-soft line-through' : 'text-wb-ink'
		]}
	>
		{parts.name}
	</span>
	<span class="min-w-0 flex-1 truncate text-[11px] text-wb-ink-soft">{parts.dir}</span>
	<div
		class="flex shrink-0 items-center gap-0.5 opacity-0 group-focus-within:opacity-100 group-hover:opacity-100"
	>
		{#if onDiscard}
			<button
				type="button"
				class={iconButton}
				aria-label="Discard changes to {parts.name}"
				title="Discard changes…"
				onclick={onDiscard}
			>
				<UndoIcon class="size-3" />
			</button>
		{/if}
		<button
			type="button"
			class={iconButton}
			aria-label="{staged ? 'Unstage' : 'Stage'} {parts.name}"
			title={staged ? 'Unstage' : 'Stage'}
			onclick={onToggle}
		>
			{#if staged}
				<MinusIcon class="size-3" />
			{:else}
				<PlusIcon class="size-3" />
			{/if}
		</button>
	</div>
	<span class="w-3 shrink-0 text-center font-mono text-[11px] font-semibold {display.color}">
		{display.letter}
	</span>
</div>
