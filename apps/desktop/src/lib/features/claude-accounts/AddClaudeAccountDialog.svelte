<script lang="ts">
	import * as Dialog from '@workbench/ui/dialog';
	import { Button } from '@workbench/ui/button';
	import { Input } from '@workbench/ui/input';
	import { homeDir } from '@tauri-apps/api/path';
	import { getWorkbenchSettingsStore } from '$stores/context';
	import { suggestConfigDir } from './claude-accounts.svelte';

	let {
		open = $bindable(false),
		onAdded
	}: {
		open?: boolean;
		/** Called with the new account, already active, so the caller can start its login. */
		onAdded: (accountId: string) => void;
	} = $props();

	const settings = getWorkbenchSettingsStore();

	let home = $state('');
	homeDir()
		.then((h) => (home = h))
		.catch(() => {});

	let name = $state('');
	// Follows the name until edited by hand.
	let configDir = $derived(home ? suggestConfigDir(home, name) : '');
	let error = $state('');
	let saving = $state(false);

	function reset() {
		name = '';
		error = '';
	}

	async function add() {
		saving = true;
		error = '';
		try {
			const account = await settings.addClaudeAccount(name, configDir);
			await settings.setActiveClaudeAccount(account.id);
			open = false;
			reset();
			onAdded(account.id);
		} catch (e) {
			error = e instanceof Error ? e.message : String(e);
		} finally {
			saving = false;
		}
	}
</script>

<Dialog.Root bind:open onOpenChange={(o) => !o && reset()}>
	<Dialog.Content class="max-w-sm">
		<Dialog.Header>
			<Dialog.Title>Add Claude account</Dialog.Title>
			<Dialog.Description>
				Each account keeps its own login, settings and session history in its own config folder.
				You'll log in next.
			</Dialog.Description>
		</Dialog.Header>

		<form
			class="flex flex-col gap-3 py-2"
			onsubmit={(e) => {
				e.preventDefault();
				add();
			}}
		>
			<label class="flex flex-col gap-1 text-sm">
				Name
				<Input bind:value={name} placeholder="Work" />
			</label>
			<label class="flex flex-col gap-1 text-sm">
				Config folder
				<Input bind:value={configDir} class="font-mono text-xs" />
			</label>
			{#if error}
				<p class="text-xs text-wb-err">{error}</p>
			{/if}

			<Dialog.Footer>
				<Button type="button" variant="ghost" onclick={() => (open = false)}>Cancel</Button>
				<Button type="submit" disabled={saving || !name.trim()}>
					{saving ? 'Adding…' : 'Add and log in'}
				</Button>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>
