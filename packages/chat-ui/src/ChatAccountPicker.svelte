<script lang="ts">
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import * as DropdownMenu from '@workbench/ui/dropdown-menu';
	import type { ClaudeAccount } from '@workbench/types';
	import { accountChoices } from './chat-format';

	let {
		accounts,
		accountId,
		disabled,
		onAccount
	}: {
		/** The extra Claude accounts; the default one is always offered. */
		accounts: Pick<ClaudeAccount, 'id' | 'name'>[];
		accountId: string | undefined;
		disabled: boolean;
		onAccount: (accountId: string | undefined) => void;
	} = $props();

	const choices = $derived(accountChoices(accounts));
	const current = $derived(choices.find((c) => c.id === accountId) ?? choices[0]);
</script>

<DropdownMenu.Root>
	<DropdownMenu.Trigger>
		{#snippet child({ props })}
			<button
				{...props}
				type="button"
				{disabled}
				class="flex min-w-0 items-center gap-1 rounded-md px-2 py-1 text-xs whitespace-nowrap text-wb-ink-mute hover:bg-wb-panel2 hover:text-wb-ink focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none disabled:opacity-50"
				title="Claude account"
			>
				<span class="truncate">{current.name}</span>
				<ChevronDownIcon class="size-3 shrink-0" />
			</button>
		{/snippet}
	</DropdownMenu.Trigger>
	<DropdownMenu.Content align="start" class="w-64">
		<DropdownMenu.Label class="text-[11px] font-normal text-muted-foreground">
			Continue this chat on another account. It restarts there and re-reads the conversation, so the
			next reply starts with a cold cache.
		</DropdownMenu.Label>
		<DropdownMenu.RadioGroup
			value={current.key}
			onValueChange={(key) => onAccount(choices.find((c) => c.key === key)?.id)}
		>
			{#each choices as choice (choice.key)}
				<DropdownMenu.RadioItem value={choice.key}>{choice.name}</DropdownMenu.RadioItem>
			{/each}
		</DropdownMenu.RadioGroup>
	</DropdownMenu.Content>
</DropdownMenu.Root>
