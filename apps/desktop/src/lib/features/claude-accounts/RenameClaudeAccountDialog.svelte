<script lang="ts">
	import * as Dialog from '@workbench/ui/dialog';
	import { Button } from '@workbench/ui/button';
	import { Input } from '@workbench/ui/input';
	import { getWorkbenchSettingsStore } from '$stores/context';

	let {
		account = $bindable(null)
	}: {
		/** The account being renamed (no id: the default `~/.claude` one); null closes the dialog. */
		account?: { id: string | undefined; name: string } | null;
	} = $props();

	const settings = getWorkbenchSettingsStore();

	// Seeded from the account each time the dialog opens on one.
	let name = $derived(account?.name ?? '');
	let error = $state('');
	let saving = $state(false);

	function close() {
		account = null;
		error = '';
	}

	async function rename() {
		if (!account) return;
		saving = true;
		error = '';
		try {
			await settings.renameClaudeAccount(account.id, name);
			close();
		} catch (e) {
			error = e instanceof Error ? e.message : String(e);
		} finally {
			saving = false;
		}
	}
</script>

<Dialog.Root open={account !== null} onOpenChange={(open) => !open && close()}>
	<Dialog.Content class="max-w-sm">
		<Dialog.Header>
			<Dialog.Title>Rename Claude account</Dialog.Title>
			<Dialog.Description
				>Only the name Workbench shows changes; the login stays.</Dialog.Description
			>
		</Dialog.Header>

		<form
			class="flex flex-col gap-3 py-2"
			onsubmit={(e) => {
				e.preventDefault();
				rename();
			}}
		>
			<label class="flex flex-col gap-1 text-sm">
				Name
				<Input bind:value={name} />
			</label>
			{#if error}
				<p class="text-xs text-wb-err">{error}</p>
			{/if}

			<Dialog.Footer>
				<Button type="button" variant="ghost" onclick={close}>Cancel</Button>
				<Button type="submit" disabled={saving || !name.trim()}>
					{saving ? 'Saving…' : 'Rename'}
				</Button>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>
