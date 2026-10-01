<script lang="ts">
	import { Input } from '@workbench/ui/input';
	import { getClaudeSettingsStore } from '$stores/context';
	import EditableStringList from './EditableStringList.svelte';
	import SettingsRow from './SettingsRow.svelte';
	import SettingsSection from './SettingsSection.svelte';
	import SettingsToggle from './SettingsToggle.svelte';

	const claudeSettingsStore = getClaudeSettingsStore();

	let settings = $derived(claudeSettingsStore.currentSettings);
	let sandbox = $derived(settings.sandbox ?? {});
	let network = $derived(sandbox.network ?? {});

	let excludedCommands = $derived((sandbox.excludedCommands ?? []) as string[]);
	let allowedDomains = $derived((network.allowedDomains ?? []) as string[]);
	let allowUnixSockets = $derived((network.allowUnixSockets ?? []) as string[]);

	const proxyPorts = [
		{ key: 'httpProxyPort', label: 'HTTP proxy port', placeholder: 'e.g. 8080' },
		{ key: 'socksProxyPort', label: 'SOCKS proxy port', placeholder: 'e.g. 1080' }
	] as const;
</script>

<SettingsSection title="Sandbox">
	<SettingsToggle
		label="Sandbox enabled"
		description="Run Bash commands in an isolated sandbox."
		checked={sandbox.enabled ?? false}
		onCheckedChange={(v) => claudeSettingsStore.updateSandbox({ enabled: v })}
	/>

	{#if sandbox.enabled}
		<SettingsToggle
			label="Auto-allow sandboxed Bash"
			description="Skip the permission prompt for Bash commands while sandboxed."
			checked={sandbox.autoAllowBashIfSandboxed ?? false}
			onCheckedChange={(v) => claudeSettingsStore.updateSandbox({ autoAllowBashIfSandboxed: v })}
		/>

		<SettingsToggle
			label="Allow unsandboxed commands"
			description="Permit commands that cannot run inside the sandbox."
			checked={sandbox.allowUnsandboxedCommands ?? false}
			onCheckedChange={(v) => claudeSettingsStore.updateSandbox({ allowUnsandboxedCommands: v })}
		/>

		<SettingsToggle
			label="Weaker nested sandbox"
			description="Use a less restrictive sandbox for nested operations."
			checked={sandbox.enableWeakerNestedSandbox ?? false}
			onCheckedChange={(v) => claudeSettingsStore.updateSandbox({ enableWeakerNestedSandbox: v })}
		/>

		<SettingsRow label="Excluded commands" description="Commands that skip the sandbox." stack>
			{#snippet control()}
				<EditableStringList
					items={excludedCommands}
					onAdd={(v) => claudeSettingsStore.addToSandboxList('excludedCommands', v)}
					onRemove={(v) => claudeSettingsStore.removeFromSandboxList('excludedCommands', v)}
					placeholder="e.g. docker"
				/>
			{/snippet}
		</SettingsRow>
	{/if}
</SettingsSection>

{#if sandbox.enabled}
	<SettingsSection title="Network">
		<SettingsRow label="Allowed domains" description="Domains sandboxed commands can reach." stack>
			{#snippet control()}
				<EditableStringList
					items={allowedDomains}
					onAdd={(v) => claudeSettingsStore.addToSandboxNetworkList('allowedDomains', v)}
					onRemove={(v) => claudeSettingsStore.removeFromSandboxNetworkList('allowedDomains', v)}
					placeholder="e.g. api.github.com"
				/>
			{/snippet}
		</SettingsRow>

		<SettingsToggle
			label="Allow local binding"
			description="Let sandboxed commands listen on local ports."
			checked={network.allowLocalBinding ?? false}
			onCheckedChange={(v) => claudeSettingsStore.updateSandboxNetwork({ allowLocalBinding: v })}
		/>

		<SettingsToggle
			label="Allow all Unix sockets"
			description="Permit every Unix domain socket."
			checked={network.allowAllUnixSockets ?? false}
			onCheckedChange={(v) => claudeSettingsStore.updateSandboxNetwork({ allowAllUnixSockets: v })}
		/>

		{#if !network.allowAllUnixSockets}
			<SettingsRow
				label="Allowed Unix sockets"
				description="Specific socket paths sandboxed commands can use."
				stack
			>
				{#snippet control()}
					<EditableStringList
						items={allowUnixSockets}
						onAdd={(v) => claudeSettingsStore.addToSandboxNetworkList('allowUnixSockets', v)}
						onRemove={(v) =>
							claudeSettingsStore.removeFromSandboxNetworkList('allowUnixSockets', v)}
						placeholder="e.g. /var/run/docker.sock"
					/>
				{/snippet}
			</SettingsRow>
		{/if}

		{#each proxyPorts as proxy (proxy.key)}
			<SettingsRow label={proxy.label}>
				{#snippet control()}
					<Input
						type="number"
						class="h-8 w-28 font-mono text-xs"
						placeholder={proxy.placeholder}
						aria-label={proxy.label}
						value={network[proxy.key]?.toString() ?? ''}
						oninput={(e) => {
							const val = e.currentTarget.value;
							claudeSettingsStore.updateSandboxNetwork({
								[proxy.key]: val ? parseInt(val, 10) : undefined
							});
						}}
					/>
				{/snippet}
			</SettingsRow>
		{/each}
	</SettingsSection>
{/if}
