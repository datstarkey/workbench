<script lang="ts">
	import { onMount } from 'svelte';
	import { formatElapsed } from './chat-format';

	/** A live `m:ss` timer counting from `since` (ms). */
	let { since }: { since: number } = $props();

	let now = $state(Date.now());
	onMount(() => {
		const timer = setInterval(() => (now = Date.now()), 1000);
		return () => clearInterval(timer);
	});
</script>

<span class="tabular-nums">{formatElapsed(now - since)}</span>
