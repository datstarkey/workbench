<script lang="ts">
	import { AgentIcon } from '@workbench/ui/agent-icon';
	import { SETTINGS_NAV, type SettingsPageId } from './settings-pages';

	let {
		active,
		onSelect
	}: {
		active: SettingsPageId;
		onSelect: (id: SettingsPageId) => void;
	} = $props();
</script>

<nav
	class="flex w-[200px] shrink-0 flex-col gap-px overflow-y-auto border-r border-wb-hair bg-wb-bg px-2 py-3.5 select-none"
	aria-label="Settings sections"
>
	{#each SETTINGS_NAV as group, i (group.label)}
		{#if group.sub}
			<div
				class="flex items-center gap-2 px-2.5 pt-2.5 pb-1 font-mono text-[10.5px] text-wb-ink-mute after:h-px after:flex-1 after:bg-wb-hair"
			>
				{group.label}
			</div>
		{:else}
			<div
				class={[
					'flex items-center gap-2 px-2.5 pb-1.5 text-[10.5px] font-semibold tracking-wider text-wb-ink-mute uppercase',
					i === 0 ? 'pt-0.5' : 'pt-3.5'
				]}
			>
				{#if group.label === 'Claude Code'}<AgentIcon
						agent="claude"
						class="size-3.5"
					/>{:else if group.label === 'Codex'}<AgentIcon agent="codex" class="size-3.5" />{/if}
				{group.label}
			</div>
		{/if}
		{#each group.pages as page (page.id)}
			<button
				type="button"
				aria-current={active === page.id ? 'page' : undefined}
				class={[
					'rounded-md px-2.5 py-1.5 text-left text-[12.5px] transition-colors',
					active === page.id
						? 'bg-wb-panel2 text-wb-ink'
						: 'text-wb-ink-mute hover:bg-wb-panel hover:text-wb-ink'
				]}
				onclick={() => onSelect(page.id)}
			>
				{page.label}
			</button>
		{/each}
	{/each}
</nav>
