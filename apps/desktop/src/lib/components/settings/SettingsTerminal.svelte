<script lang="ts">
	import { onMount } from 'svelte';
	import { getWorkbenchSettingsStore } from '$stores/context';
	import { isNativeTerminalAvailable } from '$lib/utils/terminal';
	import type { TerminalPerformanceMode, TerminalRenderer } from '$types/workbench';
	import SettingsSection from './SettingsSection.svelte';
	import SettingsSelect from './SettingsSelect.svelte';
	import SettingsToggle from './SettingsToggle.svelte';

	const store = getWorkbenchSettingsStore();

	let nativeAvailable = $state(false);

	onMount(async () => {
		try {
			nativeAvailable = await isNativeTerminalAvailable();
		} catch {
			nativeAvailable = false;
		}
	});

	const rendererOptions = [
		{ value: 'xterm', label: 'xterm.js (Web)' },
		{ value: 'native', label: 'Native (SwiftTerm)' }
	];
	const perfModeOptions = [
		{ value: 'auto', label: 'Background panes only' },
		{ value: 'always', label: 'All panes' }
	];
</script>

<SettingsSection title="Rendering">
	{#if nativeAvailable}
		<SettingsSelect
			label="Renderer"
			description="Native uses macOS SwiftTerm and has no split panes. Applies to new workspaces."
			options={rendererOptions}
			value={store.terminalRenderer}
			onValueChange={(v) => store.set('terminalRenderer', v as TerminalRenderer)}
		/>
	{/if}

	<SettingsSelect
		label="Throughput mode"
		description="Trades latency for throughput. Hidden panes always use it."
		options={perfModeOptions}
		value={store.terminalPerformanceMode}
		onValueChange={(v) => store.set('terminalPerformanceMode', v as TerminalPerformanceMode)}
	/>
</SettingsSection>

<SettingsSection title="Diagnostics">
	<SettingsToggle
		label="Terminal telemetry"
		description="Log throughput and latency to the developer console."
		checked={store.terminalTelemetryEnabled}
		onCheckedChange={(checked) => store.set('terminalTelemetryEnabled', checked)}
	/>
</SettingsSection>
