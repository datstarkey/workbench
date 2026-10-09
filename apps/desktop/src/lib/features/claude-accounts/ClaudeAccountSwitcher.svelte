<script lang="ts">
	import { AgentIcon } from '@workbench/ui/agent-icon';
	import * as DropdownMenu from '@workbench/ui/dropdown-menu';
	import CheckIcon from '@lucide/svelte/icons/check';
	import LogInIcon from '@lucide/svelte/icons/log-in';
	import PencilIcon from '@lucide/svelte/icons/pencil';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import TrashIcon from '@lucide/svelte/icons/trash-2';
	import { getWorkbenchSettingsStore, getWorkspaceStore } from '$stores/context';
	import AddClaudeAccountDialog from './AddClaudeAccountDialog.svelte';
	import RenameClaudeAccountDialog from './RenameClaudeAccountDialog.svelte';
	import {
		ClaudeAccountStatuses,
		DEFAULT_ACCOUNT_KEY,
		LOGIN_TASK,
		describeAuth,
		describeUsage,
		isHighUsage,
		usageDetail
	} from './claude-accounts.svelte';

	const settings = getWorkbenchSettingsStore();
	const workspaceStore = getWorkspaceStore();
	const statuses = new ClaudeAccountStatuses();

	let addOpen = $state(false);
	let renaming = $state<{ id: string | undefined; name: string } | null>(null);

	const activeId = $derived(settings.activeClaudeAccountId);
	const accounts = $derived([
		{ id: undefined, name: settings.defaultAccountName, detail: '~/.claude' },
		...settings.claudeAccounts.map((a) => ({ id: a.id, name: a.name, detail: a.configDir }))
	]);
	const active = $derived(accounts.find((a) => a.id === activeId) ?? accounts[0]);
	const canLogin = $derived(workspaceStore.activeWorkspace !== null);

	/** Opens `claude auth login` in a new tab of the active workspace, under that account. */
	function login(accountId: string | undefined) {
		const ws = workspaceStore.activeWorkspace;
		if (ws) workspaceStore.runTaskInWorkspace(ws.id, LOGIN_TASK, accountId);
	}
</script>

<DropdownMenu.Root onOpenChange={(open) => open && statuses.refresh(settings.claudeAccounts)}>
	<DropdownMenu.Trigger
		class="flex items-center gap-1 rounded px-1 transition-colors hover:text-wb-ink"
		title="Claude account for new sessions"
	>
		<AgentIcon agent="claude" class="size-3" />
		{active.name}
	</DropdownMenu.Trigger>

	<DropdownMenu.Content class="min-w-60" align="end" side="top">
		<DropdownMenu.Label class="text-xs text-wb-ink-soft">
			New Claude sessions use
		</DropdownMenu.Label>
		{#each accounts as account (account.id ?? DEFAULT_ACCOUNT_KEY)}
			{@const key = account.id ?? DEFAULT_ACCOUNT_KEY}
			{@const usage = statuses.usageByKey[key]}
			{@const usageLabel = describeUsage(usage)}
			<DropdownMenu.Item onSelect={() => settings.setActiveClaudeAccount(account.id ?? null)}>
				<CheckIcon class={['size-3.5', account.id !== activeId && 'invisible']} />
				<span class="flex min-w-0 flex-1 flex-col">
					<span class="truncate">{account.name}</span>
					<span class="truncate text-[10px] text-wb-ink-soft" title={account.detail}>
						{describeAuth(statuses.authByKey[key])}
					</span>
					{#if usageLabel}
						<span
							class={[
								'truncate text-[10px]',
								isHighUsage(usage) ? 'text-wb-warn' : 'text-wb-ink-soft'
							]}
							title={usageDetail(usage)}
						>
							{usageLabel}
						</span>
					{/if}
				</span>
			</DropdownMenu.Item>
		{/each}

		<DropdownMenu.Separator />
		<DropdownMenu.Item disabled={!canLogin} onSelect={() => login(activeId)}>
			<LogInIcon class="size-3.5" />
			<span>{canLogin ? `Log in to ${active.name}…` : 'Open a project to log in'}</span>
		</DropdownMenu.Item>
		<DropdownMenu.Sub>
			<DropdownMenu.SubTrigger>
				<PencilIcon class="size-3.5" />
				<span>Rename account</span>
			</DropdownMenu.SubTrigger>
			<DropdownMenu.SubContent class="min-w-40">
				{#each accounts as account (account.id ?? DEFAULT_ACCOUNT_KEY)}
					<DropdownMenu.Item onSelect={() => (renaming = { id: account.id, name: account.name })}>
						<span class="truncate">{account.name}…</span>
					</DropdownMenu.Item>
				{/each}
			</DropdownMenu.SubContent>
		</DropdownMenu.Sub>
		<DropdownMenu.Item onSelect={() => (addOpen = true)}>
			<PlusIcon class="size-3.5" />
			<span>Add account…</span>
		</DropdownMenu.Item>
		{#if activeId}
			<DropdownMenu.Item
				class="text-wb-err"
				onSelect={() => settings.removeClaudeAccount(activeId)}
				title="Forgets the account; its config folder stays on disk"
			>
				<TrashIcon class="size-3.5" />
				<span>Remove {active.name}</span>
			</DropdownMenu.Item>
		{/if}
	</DropdownMenu.Content>
</DropdownMenu.Root>

<AddClaudeAccountDialog bind:open={addOpen} onAdded={login} />
<RenameClaudeAccountDialog bind:account={renaming} />
