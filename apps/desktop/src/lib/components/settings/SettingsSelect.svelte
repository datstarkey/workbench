<script lang="ts">
	import type { Snippet } from 'svelte';
	import * as Select from '@workbench/ui/select';
	import SettingsRow from './SettingsRow.svelte';

	let {
		label,
		description,
		options,
		value,
		onValueChange,
		triggerClass = 'w-44',
		children
	}: {
		label: string;
		description?: string | Snippet;
		options: Array<{ value: string; label: string }>;
		value: string;
		onValueChange: (value: string) => void;
		triggerClass?: string;
		children?: Snippet;
	} = $props();
</script>

<SettingsRow {label} {description} {children}>
	{#snippet control()}
		<Select.Root type="single" {value} {onValueChange}>
			<Select.Trigger class={triggerClass} aria-label={label}>
				{options.find((o) => o.value === value)?.label ?? value}
			</Select.Trigger>
			<Select.Content>
				{#each options as opt (opt.value)}
					<Select.Item value={opt.value}>{opt.label}</Select.Item>
				{/each}
			</Select.Content>
		</Select.Root>
	{/snippet}
</SettingsRow>
