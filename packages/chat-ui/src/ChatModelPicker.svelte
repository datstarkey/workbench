<script lang="ts">
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import * as DropdownMenu from '@workbench/ui/dropdown-menu';
	import type { EffortLevel, TranscriptMeta } from '@workbench/types';
	import { canPickModel, currentModel, effortLabel } from './chat-format';

	let {
		meta,
		disabled,
		onModel,
		onEffort
	}: {
		meta: TranscriptMeta | null;
		disabled: boolean;
		onModel: (model: string) => void;
		onEffort: (effort: EffortLevel) => void;
	} = $props();

	const model = $derived(currentModel(meta));
	const levels = $derived(model?.effortLevels ?? []);
	/** A picked level the new model doesn't take falls back to its default. */
	const effort = $derived(meta?.effort && levels.includes(meta.effort) ? meta.effort : null);

	const trigger =
		'flex min-w-0 items-center gap-1 rounded-md px-2 py-1 text-xs whitespace-nowrap text-wb-ink-mute hover:bg-wb-panel2 hover:text-wb-ink focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none disabled:opacity-50';
</script>

{#if meta && meta.models.length > 0}
	<DropdownMenu.Root>
		<DropdownMenu.Trigger>
			{#snippet child({ props })}
				<button {...props} type="button" {disabled} class={trigger} title="Model">
					<span class="truncate">{model?.displayName ?? 'Model'}</span>
					<ChevronDownIcon class="size-3 shrink-0" />
				</button>
			{/snippet}
		</DropdownMenu.Trigger>
		<DropdownMenu.Content align="start" class="w-72">
			<DropdownMenu.RadioGroup value={model?.value ?? ''} onValueChange={(value) => onModel(value)}>
				{#each meta.models as option (option.value)}
					<DropdownMenu.RadioItem
						value={option.value}
						disabled={!canPickModel(option, model)}
						class="flex-col items-start gap-0"
					>
						<span>{option.displayName}</span>
						{#if option.description}
							<span class="text-[11px] text-muted-foreground">{option.description}</span>
						{/if}
					</DropdownMenu.RadioItem>
				{/each}
			</DropdownMenu.RadioGroup>
		</DropdownMenu.Content>
	</DropdownMenu.Root>

	{#if levels.length > 0}
		<DropdownMenu.Root>
			<DropdownMenu.Trigger>
				{#snippet child({ props })}
					<button {...props} type="button" {disabled} class={trigger} title="Effort">
						<span class="truncate">{effortLabel(effort)}</span>
						<ChevronDownIcon class="size-3 shrink-0" />
					</button>
				{/snippet}
			</DropdownMenu.Trigger>
			<DropdownMenu.Content align="start" class="w-48">
				<DropdownMenu.RadioGroup
					value={effort ?? ''}
					onValueChange={(value) => onEffort(value as EffortLevel)}
				>
					{#each levels as level (level)}
						<DropdownMenu.RadioItem value={level}>{effortLabel(level)}</DropdownMenu.RadioItem>
					{/each}
				</DropdownMenu.RadioGroup>
			</DropdownMenu.Content>
		</DropdownMenu.Root>
	{/if}
{/if}
