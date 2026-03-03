<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import type { SearchAddon } from '@xterm/addon-search';
	import { searchDecorations } from '$lib/terminal-config';
	import ChevronUpIcon from '@lucide/svelte/icons/chevron-up';
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import XIcon from '@lucide/svelte/icons/x';

	let { searchAddon, onClose }: { searchAddon: SearchAddon; onClose: () => void } = $props();

	let searchTerm = $state('');
	let caseSensitive = $state(false);
	let regex = $state(false);
	let wholeWord = $state(false);
	let resultIndex = $state(-1);
	let resultCount = $state(0);
	let inputEl: HTMLInputElement;
	let resultsDisposable: { dispose(): void } | null = null;

	function searchOptions(incremental: boolean) {
		return {
			regex,
			wholeWord,
			caseSensitive,
			incremental,
			decorations: searchDecorations
		};
	}

	function findNext(incremental: boolean) {
		if (!searchTerm) return;
		searchAddon.findNext(searchTerm, searchOptions(incremental));
	}

	function findPrevious() {
		if (!searchTerm) return;
		searchAddon.findPrevious(searchTerm, searchOptions(false));
	}

	function close() {
		searchAddon.clearDecorations();
		onClose();
	}

	function onInput() {
		findNext(true);
	}

	function onKeydown(event: KeyboardEvent) {
		if (event.key === 'Enter') {
			event.preventDefault();
			if (event.shiftKey) {
				findPrevious();
			} else {
				findNext(false);
			}
		}
		if (event.key === 'Escape') {
			event.preventDefault();
			close();
		}
	}

	function toggleOption(option: 'caseSensitive' | 'regex' | 'wholeWord') {
		if (option === 'caseSensitive') caseSensitive = !caseSensitive;
		else if (option === 'regex') regex = !regex;
		else wholeWord = !wholeWord;
		if (searchTerm) findNext(true);
	}

	onMount(() => {
		resultsDisposable = searchAddon.onDidChangeResults((e) => {
			resultIndex = e.resultIndex;
			resultCount = e.resultCount;
		});
		inputEl?.focus();
	});

	onDestroy(() => {
		resultsDisposable?.dispose();
		searchAddon.clearDecorations();
	});
</script>

<div
	class="absolute top-1 right-2 z-10 flex items-center gap-1 rounded border border-border/60 bg-background/95 px-2 py-1 shadow-lg backdrop-blur-sm"
>
	<input
		bind:this={inputEl}
		bind:value={searchTerm}
		oninput={onInput}
		onkeydown={onKeydown}
		type="text"
		placeholder="Search..."
		class="h-6 w-48 rounded border border-border/40 bg-transparent px-1.5 text-xs text-foreground outline-none focus:border-primary/60"
	/>

	<span class="min-w-[3.5rem] text-center text-[10px] text-muted-foreground tabular-nums">
		{#if searchTerm && resultCount > 0}
			{resultIndex + 1}/{resultCount}
		{:else if searchTerm}
			0/0
		{/if}
	</span>

	<button
		type="button"
		class="rounded px-1 py-0.5 text-[10px] {caseSensitive
			? 'bg-primary/20 text-foreground opacity-100'
			: 'text-muted-foreground opacity-60 hover:opacity-100'}"
		title="Match Case"
		onclick={() => toggleOption('caseSensitive')}
	>
		Aa
	</button>

	<button
		type="button"
		class="rounded px-1 py-0.5 text-[10px] {regex
			? 'bg-primary/20 text-foreground opacity-100'
			: 'text-muted-foreground opacity-60 hover:opacity-100'}"
		title="Use Regular Expression"
		onclick={() => toggleOption('regex')}
	>
		.*
	</button>

	<button
		type="button"
		class="rounded px-1 py-0.5 text-[10px] {wholeWord
			? 'bg-primary/20 text-foreground opacity-100'
			: 'text-muted-foreground opacity-60 hover:opacity-100'}"
		title="Match Whole Word"
		onclick={() => toggleOption('wholeWord')}
	>
		ab
	</button>

	<button
		type="button"
		class="rounded p-0.5 text-muted-foreground opacity-60 hover:opacity-100"
		title="Previous Match (Shift+Enter)"
		onclick={findPrevious}
	>
		<ChevronUpIcon size={14} />
	</button>

	<button
		type="button"
		class="rounded p-0.5 text-muted-foreground opacity-60 hover:opacity-100"
		title="Next Match (Enter)"
		onclick={() => findNext(false)}
	>
		<ChevronDownIcon size={14} />
	</button>

	<button
		type="button"
		class="rounded p-0.5 text-muted-foreground opacity-60 hover:opacity-100"
		title="Close (Escape)"
		onclick={close}
	>
		<XIcon size={14} />
	</button>
</div>
