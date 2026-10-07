<script lang="ts">
	import type { Snippet } from 'svelte';
	import { Switch } from '@workbench/ui/switch';
	import SettingsRow from './SettingsRow.svelte';

	let {
		label,
		description,
		checked,
		disabled = false,
		onCheckedChange,
		children
	}: {
		label: string;
		description?: string | Snippet;
		checked: boolean;
		disabled?: boolean;
		onCheckedChange: (value: boolean) => void;
		children?: Snippet;
	} = $props();
</script>

<SettingsRow {label} {description} {children}>
	{#snippet control()}
		<!-- Controlled: a change the caller refuses or fails springs back. -->
		<Switch bind:checked={() => checked, onCheckedChange} {disabled} aria-label={label} />
	{/snippet}
</SettingsRow>
