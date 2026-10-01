<script lang="ts">
	import { onMount } from 'svelte';
	import type { Attachment } from 'svelte/attachments';
	import { watch } from 'runed';
	import type { TaskOutput } from './agent-chat.svelte';
	import { formatBytes } from './chat-format';

	let {
		taskId,
		live,
		fetchOutput
	}: {
		taskId: string;
		/** Still running: keep refreshing. */
		live: boolean;
		fetchOutput: (taskId: string) => Promise<TaskOutput | null>;
	} = $props();

	const REFRESH_MS = 1500;

	let output = $state<TaskOutput | null>(null);
	let loaded = $state(false);

	onMount(() => {
		let stopped = false;
		const load = async () => {
			const next = await fetchOutput(taskId);
			if (stopped) return;
			output = next;
			loaded = true;
		};
		void load();
		const timer = setInterval(() => {
			if (live) void load();
		}, REFRESH_MS);
		return () => {
			stopped = true;
			clearInterval(timer);
		};
	});

	/** Keep the newest lines in view, like a terminal. */
	const followEnd: Attachment<HTMLPreElement> = (node) => {
		watch(
			() => output?.text,
			() => {
				node.scrollTop = node.scrollHeight;
			}
		);
	};
</script>

<div class="mt-2 flex flex-col gap-1">
	{#if !loaded}
		<span class="text-[11px] text-wb-ink-soft">Loading output…</span>
	{:else if !output || !output.text}
		<span class="text-[11px] text-wb-ink-soft">
			{live ? 'No output yet.' : 'This task wrote no output.'}
		</span>
	{:else}
		<pre
			{@attach followEnd}
			class="scrollbar-thin max-h-56 overflow-auto rounded-md border border-wb-hair bg-wb-bg px-2.5 py-2 font-mono text-[10.5px] leading-relaxed whitespace-pre-wrap text-wb-ink-mute">{output.text}</pre>
		{#if output.bytes > output.text.length}
			<span class="text-[10px] text-wb-ink-soft">
				Last {formatBytes(output.text.length)} of {formatBytes(output.bytes)}
			</span>
		{/if}
	{/if}
</div>
