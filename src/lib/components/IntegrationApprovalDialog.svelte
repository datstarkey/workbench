<script lang="ts">
	import { Button } from '$lib/components/ui/button';
	import * as Dialog from '$lib/components/ui/dialog';
	import { getIntegrationApprovalStore } from '$stores/context';

	const store = getIntegrationApprovalStore();

	let title = $derived(
		store.sessionType === 'codex' ? 'Configure Codex Integration' : 'Configure Claude Integration'
	);

	function onOpenChange(open: boolean) {
		if (!open) store.dismiss();
	}
</script>

<Dialog.Root open={store.open} {onOpenChange}>
	<Dialog.Content class="sm:max-w-md">
		<Dialog.Header>
			<Dialog.Title>{title}</Dialog.Title>
			<Dialog.Description>{store.description}</Dialog.Description>
		</Dialog.Header>
		{#if store.error}
			<p class="text-sm text-destructive">{store.error}</p>
		{/if}
		<p class="text-xs text-muted-foreground">You can change this later in Settings.</p>
		<Dialog.Footer>
			<Button type="button" variant="ghost" onclick={() => store.skip()}>Skip</Button>
			<Button type="button" onclick={() => store.approve()}>Apply</Button>
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>
