<script lang="ts">
	import type { Snippet } from 'svelte';
	import LoaderIcon from '@lucide/svelte/icons/loader';
	import { Button } from '@workbench/ui/button';
	import * as Dialog from '@workbench/ui/dialog';

	let {
		open = $bindable(),
		title = 'Are you sure?',
		description = '',
		confirmLabel = 'Confirm',
		busyLabel = confirmLabel,
		cancelLabel = 'Cancel',
		destructive = false,
		busy = false,
		error = '',
		onConfirm,
		children
	}: {
		open: boolean;
		title?: string;
		description?: string;
		confirmLabel?: string;
		busyLabel?: string;
		cancelLabel?: string;
		destructive?: boolean;
		busy?: boolean;
		error?: string;
		onConfirm: () => void;
		children?: Snippet;
	} = $props();
</script>

<Dialog.Root bind:open>
	<Dialog.Content
		class="sm:max-w-md"
		showCloseButton={!busy}
		escapeKeydownBehavior={busy ? 'ignore' : 'close'}
		interactOutsideBehavior={busy ? 'ignore' : 'close'}
	>
		<Dialog.Header>
			<Dialog.Title>{title}</Dialog.Title>
			{#if description}
				<Dialog.Description>{description}</Dialog.Description>
			{/if}
		</Dialog.Header>
		{#if children}
			{@render children()}
		{/if}
		{#if error}
			<p class="text-sm text-destructive">{error}</p>
		{/if}
		<Dialog.Footer>
			<Button type="button" variant="ghost" disabled={busy} onclick={() => (open = false)}>
				{cancelLabel}
			</Button>
			<Button
				type="button"
				variant={destructive ? 'destructive' : 'default'}
				class="gap-1.5"
				disabled={busy}
				onclick={onConfirm}
			>
				{#if busy}
					<LoaderIcon class="size-3 animate-spin" />
					{busyLabel}
				{:else}
					{confirmLabel}
				{/if}
			</Button>
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>
