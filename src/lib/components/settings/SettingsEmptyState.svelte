<script lang="ts">
	let {
		title,
		subtitle
	}: {
		title: string;
		subtitle?: string;
	} = $props();
</script>

<div class="rounded-md border border-dashed border-border/60 py-8 text-center">
	<p class="text-sm text-muted-foreground">{title}</p>
	{#if subtitle}
		<p class="mt-1 text-xs text-muted-foreground/60">{subtitle}</p>
	{/if}
</div>
