<script lang="ts">
	import PlayIcon from '@lucide/svelte/icons/play';
	import SparklesIcon from '@lucide/svelte/icons/sparkles';
	import ZapIcon from '@lucide/svelte/icons/zap';
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import { Input } from '$lib/components/ui/input';
	import * as Tooltip from '$lib/components/ui/tooltip';
	import { getClaudeSessionStore, getWorkbenchSettingsStore } from '$stores/context';
	import type { AgentAction, ProjectWorkspace } from '$types/workbench';

	const claudeSessionStore = getClaudeSessionStore();
	const workbenchSettingsStore = getWorkbenchSettingsStore();

	let {
		workspace,
		showTextButton = false
	}: {
		workspace: ProjectWorkspace;
		showTextButton?: boolean;
	} = $props();

	let search = $state('');

	let runnableActions = $derived.by(() =>
		workbenchSettingsStore.runnableActions.map((action) => {
			const preview =
				action.prompt.split('\n').find((line) => line.trim().length > 0) ?? action.prompt;
			return {
				...action,
				preview,
				category: action.category.trim(),
				tags: action.tags.map((tag) => tag.trim()).filter((tag) => tag.length > 0)
			};
		})
	);

	let filteredActions = $derived.by(() => {
		const query = search.trim().toLowerCase();
		if (!query) return runnableActions;

		return runnableActions.filter((action) => {
			const haystack = [
				action.name,
				action.prompt,
				action.preview,
				action.category,
				...action.tags
			].join(' ');
			return haystack.toLowerCase().includes(query);
		});
	});

	function runAction(action: AgentAction, type: 'claude' | 'codex') {
		claudeSessionStore.startAgentActionInWorkspace(workspace, action, type);
	}
</script>

<DropdownMenu.Root onOpenChange={(open) => !open && (search = '')}>
	{#if showTextButton}
		<DropdownMenu.Trigger>
			{#snippet child({ props })}
				<Button {...props} type="button" variant="outline">
					<PlayIcon class="size-4" />
					Run Action
				</Button>
			{/snippet}
		</DropdownMenu.Trigger>
	{:else}
		<Tooltip.Root>
			<Tooltip.Trigger>
				<DropdownMenu.Trigger>
					{#snippet child({ props })}
						<Button
							{...props}
							variant="ghost"
							size="icon-sm"
							class="size-7 text-muted-foreground hover:text-foreground"
							type="button"
						>
							<PlayIcon class="size-3.5" />
						</Button>
					{/snippet}
				</DropdownMenu.Trigger>
			</Tooltip.Trigger>
			<Tooltip.Content>Run Agent Action</Tooltip.Content>
		</Tooltip.Root>
	{/if}

	<DropdownMenu.Content align="end" class="max-h-80 w-72 overflow-y-auto">
		<DropdownMenu.Label>Agent Actions</DropdownMenu.Label>
		<DropdownMenu.Separator />
		<div class="px-2 py-1.5">
			<Input
				class="h-7 text-xs"
				placeholder="Search by name, category, prompt, or tag"
				bind:value={search}
			/>
		</div>
		<DropdownMenu.Separator />
		{#if runnableActions.length === 0}
			<div class="px-2 py-3 text-center text-xs text-muted-foreground">
				No actions configured. Add actions in Settings -> Workbench.
			</div>
		{:else if filteredActions.length === 0}
			<div class="px-2 py-3 text-center text-xs text-muted-foreground">No matching actions.</div>
		{:else}
			{#each filteredActions as action (action.id)}
				<DropdownMenu.Sub>
					<DropdownMenu.SubTrigger>
						<div class="flex min-w-0 flex-col">
							<span class="truncate text-xs font-medium">{action.name}</span>
							<span class="line-clamp-1 text-[10px] text-muted-foreground">{action.preview}</span>
							<div class="mt-1 flex flex-wrap gap-1">
								{#if action.category}
									<Badge variant="outline" class="h-4 rounded-sm px-1 text-[9px]">
										{action.category}
									</Badge>
								{/if}
								{#each action.tags.slice(0, 2) as tag (`${action.id}-${tag}`)}
									<Badge variant="secondary" class="h-4 rounded-sm px-1 text-[9px]">
										#{tag}
									</Badge>
								{/each}
							</div>
						</div>
					</DropdownMenu.SubTrigger>
					<DropdownMenu.SubContent class="w-52">
						{#if action.target !== 'codex'}
							<DropdownMenu.Item onclick={() => runAction(action, 'claude')}>
								<SparklesIcon class="size-3.5 text-violet-400" />
								Start in Claude
							</DropdownMenu.Item>
						{/if}
						{#if action.target !== 'claude'}
							<DropdownMenu.Item onclick={() => runAction(action, 'codex')}>
								<ZapIcon class="size-3.5 text-sky-400" />
								Start in Codex
							</DropdownMenu.Item>
						{/if}
					</DropdownMenu.SubContent>
				</DropdownMenu.Sub>
			{/each}
		{/if}
	</DropdownMenu.Content>
</DropdownMenu.Root>
