<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import * as Dialog from '$lib/components/ui/dialog';

	let {
		open = $bindable(),
		title = 'Are you sure?',
		description = '',
		confirmLabel = 'Confirm',
		cancelLabel = 'Cancel',
		destructive = false,
		error = '',
		onConfirm
	}: {
		open: boolean;
		title?: string;
		description?: string;
		confirmLabel?: string;
		cancelLabel?: string;
		destructive?: boolean;
		error?: string;
		onConfirm: () => void;
	} = $props();
</script>

<Dialog.Root bind:open>
	<Dialog.Content class="sm:max-w-md">
		<Dialog.Header>
			<Dialog.Title>{title}</Dialog.Title>
			{#if description}
				<Dialog.Description>{description}</Dialog.Description>
			{/if}
		</Dialog.Header>
		{#if error}
			<p class="text-sm text-destructive">{error}</p>
		{/if}
		<Dialog.Footer>
			<Button type="button" variant="ghost" onclick={() => (open = false)}>{cancelLabel}</Button>
			<Button type="button" variant={destructive ? 'destructive' : 'default'} onclick={onConfirm}>
				{confirmLabel}
			</Button>
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>
