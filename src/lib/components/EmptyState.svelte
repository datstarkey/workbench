<script lang="ts">
	import PlusIcon from '@lucide/svelte/icons/plus';
	import TerminalSquareIcon from '@lucide/svelte/icons/terminal-square';
	import { Button } from '$lib/components/ui/button';

	let {
		onAddProject
	}: {
		onAddProject: () => void;
	} = $props();
</script>

<div class="flex flex-1 items-center justify-center">
	<div class="text-center">
		<div
			class="mx-auto mb-4 flex size-12 items-center justify-center rounded-xl bg-primary/10 text-primary"
		>
			<TerminalSquareIcon class="size-6" />
		</div>
		<h2 class="text-lg font-semibold tracking-tight">Open a project to start</h2>
		<p class="mt-1 text-sm text-muted-foreground">Each project gets its own terminal workspace.</p>
		<Button type="button" class="mt-4" onclick={onAddProject}>
			<PlusIcon class="size-4" />
			Add Project
		</Button>
	</div>
</div>
