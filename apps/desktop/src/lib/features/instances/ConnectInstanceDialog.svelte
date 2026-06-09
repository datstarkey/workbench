<script lang="ts">
	import * as Dialog from '@workbench/ui/dialog';
	import { Button } from '@workbench/ui/button';
	import { Input } from '@workbench/ui/input';
	import { getInstancesStore } from '$stores/context';

	let { open = $bindable(false) }: { open?: boolean } = $props();

	const instances = getInstancesStore();

	let name = $state('');
	let url = $state('http://localhost:4317');
	let token = $state('');
	let testState = $state<'idle' | 'testing' | 'ok' | 'fail'>('idle');
	let testMessage = $state('');

	function reset() {
		name = '';
		url = 'http://localhost:4317';
		token = '';
		testState = 'idle';
		testMessage = '';
	}

	async function test() {
		const base = url.trim().replace(/\/$/, '');
		if (!base) return;
		testState = 'testing';
		testMessage = '';
		try {
			const headers: Record<string, string> = {};
			if (token.trim()) headers.authorization = `Bearer ${token.trim()}`;
			const res = await fetch(`${base}/health`, { headers });
			testState = res.ok ? 'ok' : 'fail';
			if (!res.ok) testMessage = `${res.status} ${res.statusText}`;
		} catch (e) {
			testState = 'fail';
			testMessage = e instanceof Error ? e.message : String(e);
		}
	}

	function add() {
		const base = url.trim().replace(/\/$/, '');
		if (!base) return;
		const instance = instances.add({
			name: name.trim() || base.replace(/^https?:\/\//, ''),
			url: base,
			token: token.trim() || undefined
		});
		instances.setActive(instance.config.id);
		open = false;
		reset();
	}
</script>

<Dialog.Root bind:open onOpenChange={(o) => !o && reset()}>
	<Dialog.Content class="max-w-sm">
		<Dialog.Header>
			<Dialog.Title>Connect instance</Dialog.Title>
			<Dialog.Description>
				Connect to a Workbench server to manage its projects and spawn sessions there.
			</Dialog.Description>
		</Dialog.Header>

		<div class="flex flex-col gap-3 py-2">
			<label class="flex flex-col gap-1 text-sm">
				Name
				<Input bind:value={name} placeholder="linux-box" />
			</label>
			<label class="flex flex-col gap-1 text-sm">
				Server URL
				<Input bind:value={url} placeholder="http://host:4317" />
			</label>
			<label class="flex flex-col gap-1 text-sm">
				Token (optional)
				<Input bind:value={token} type="password" placeholder="bearer token" />
			</label>

			<div class="flex items-center gap-2">
				<Button variant="secondary" size="sm" onclick={test} disabled={testState === 'testing'}>
					{testState === 'testing' ? 'Testing…' : 'Test connection'}
				</Button>
				{#if testState === 'ok'}
					<span class="text-xs text-wb-ok">● reachable</span>
				{:else if testState === 'fail'}
					<span class="text-xs text-wb-err">● {testMessage || 'unreachable'}</span>
				{/if}
			</div>
		</div>

		<Dialog.Footer>
			<Button variant="ghost" onclick={() => (open = false)}>Cancel</Button>
			<Button onclick={add}>Add</Button>
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>
