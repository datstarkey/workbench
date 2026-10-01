<script lang="ts">
	import type { Snippet } from 'svelte';
	import { Switch } from '@workbench/ui/switch';
	import SettingsRow from './SettingsRow.svelte';

	let {
		label,
		description,
		checked,
		onCheckedChange,
		children
	}: {
		label: string;
		description?: string | Snippet;
		checked: boolean;
		onCheckedChange: (value: boolean) => void;
		children?: Snippet;
	} = $props();
</script>

<SettingsRow {label} {description} {children}>
	{#snippet control()}
		<Switch {checked} {onCheckedChange} aria-label={label} />
	{/snippet}
</SettingsRow>
