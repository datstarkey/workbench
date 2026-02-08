<script lang="ts">
	import { Label } from '$lib/components/ui/label';
	import { Input } from '$lib/components/ui/input';
	import { getClaudeSettingsStore } from '$stores/context';
	import EditableStringList from './EditableStringList.svelte';

	const claudeSettingsStore = getClaudeSettingsStore();
	import SettingsToggle from './SettingsToggle.svelte';

	let settings = $derived(claudeSettingsStore.currentSettings);
	let sandbox = $derived(settings.sandbox ?? {});
	let network = $derived(sandbox.network ?? {});

	let excludedCommands = $derived((sandbox.excludedCommands ?? []) as string[]);
	let allowedDomains = $derived((network.allowedDomains ?? []) as string[]);
	let allowUnixSockets = $derived((network.allowUnixSockets ?? []) as string[]);
</script>

<div class="space-y-6">
	<SettingsToggle
		label="Sandbox Enabled"
		description="Run commands in an isolated sandbox environment."
		checked={sandbox.enabled ?? false}
		onCheckedChange={(v) => claudeSettingsStore.updateSandbox({ enabled: v })}
	/>

	{#if sandbox.enabled}
		<SettingsToggle
			label="Auto-allow Bash if Sandboxed"
			description="Automatically allow bash commands when sandbox is active."
			checked={sandbox.autoAllowBashIfSandboxed ?? false}
			onCheckedChange={(v) => claudeSettingsStore.updateSandbox({ autoAllowBashIfSandboxed: v })}
		/>

		<SettingsToggle
			label="Allow Unsandboxed Commands"
			description="Permit commands that cannot run inside the sandbox."
			checked={sandbox.allowUnsandboxedCommands ?? false}
			onCheckedChange={(v) => claudeSettingsStore.updateSandbox({ allowUnsandboxedCommands: v })}
		/>

		<SettingsToggle
			label="Enable Weaker Nested Sandbox"
			description="Use a less restrictive sandbox for nested operations."
			checked={sandbox.enableWeakerNestedSandbox ?? false}
			onCheckedChange={(v) => claudeSettingsStore.updateSandbox({ enableWeakerNestedSandbox: v })}
		/>

		<div>
			<Label class="text-sm font-medium">Excluded Commands</Label>
			<p class="mt-1 text-xs text-muted-foreground">Commands excluded from sandbox restrictions.</p>
			<div class="mt-2">
				<EditableStringList
					items={excludedCommands}
					onAdd={(v) => claudeSettingsStore.addToSandboxList('excludedCommands', v)}
					onRemove={(v) => claudeSettingsStore.removeFromSandboxList('excludedCommands', v)}
					placeholder="e.g. docker"
				/>
			</div>
		</div>

		<div class="space-y-6">
			<h3 class="text-sm font-medium">Network Isolation</h3>

			<SettingsToggle
				label="Allow Local Binding"
				description="Allow the sandbox to bind to local network ports."
				checked={network.allowLocalBinding ?? false}
				onCheckedChange={(v) => claudeSettingsStore.updateSandboxNetwork({ allowLocalBinding: v })}
			/>

			<SettingsToggle
				label="Allow All Unix Sockets"
				description="Permit access to all Unix domain sockets."
				checked={network.allowAllUnixSockets ?? false}
				onCheckedChange={(v) =>
					claudeSettingsStore.updateSandboxNetwork({ allowAllUnixSockets: v })}
			/>

			<div class="grid grid-cols-2 gap-4">
				<div>
					<Label class="text-sm font-medium">HTTP Proxy Port</Label>
					<p class="mt-1 text-xs text-muted-foreground">Port for HTTP proxy.</p>
					<Input
						type="number"
						class="mt-2 h-7 w-28 font-mono text-xs"
						placeholder="e.g. 8080"
						value={network.httpProxyPort?.toString() ?? ''}
						oninput={(e) => {
							const val = (e.target as HTMLInputElement).value;
							const port = val ? parseInt(val, 10) : undefined;
							claudeSettingsStore.updateSandboxNetwork({ httpProxyPort: port });
						}}
					/>
				</div>
				<div>
					<Label class="text-sm font-medium">SOCKS Proxy Port</Label>
					<p class="mt-1 text-xs text-muted-foreground">Port for SOCKS proxy.</p>
					<Input
						type="number"
						class="mt-2 h-7 w-28 font-mono text-xs"
						placeholder="e.g. 1080"
						value={network.socksProxyPort?.toString() ?? ''}
						oninput={(e) => {
							const val = (e.target as HTMLInputElement).value;
							const port = val ? parseInt(val, 10) : undefined;
							claudeSettingsStore.updateSandboxNetwork({ socksProxyPort: port });
						}}
					/>
				</div>
			</div>

			<div>
				<Label class="text-sm font-medium">Allowed Domains</Label>
				<p class="mt-1 text-xs text-muted-foreground">Network domains the sandbox can access.</p>
				<div class="mt-2">
					<EditableStringList
						items={allowedDomains}
						onAdd={(v) => claudeSettingsStore.addToSandboxNetworkList('allowedDomains', v)}
						onRemove={(v) => claudeSettingsStore.removeFromSandboxNetworkList('allowedDomains', v)}
						placeholder="e.g. api.github.com"
					/>
				</div>
			</div>

			{#if !network.allowAllUnixSockets}
				<div>
					<Label class="text-sm font-medium">Allowed Unix Sockets</Label>
					<p class="mt-1 text-xs text-muted-foreground">
						Specific Unix socket paths the sandbox can access.
					</p>
					<div class="mt-2">
						<EditableStringList
							items={allowUnixSockets}
							onAdd={(v) => claudeSettingsStore.addToSandboxNetworkList('allowUnixSockets', v)}
							onRemove={(v) =>
								claudeSettingsStore.removeFromSandboxNetworkList('allowUnixSockets', v)}
							placeholder="e.g. /var/run/docker.sock"
						/>
					</div>
				</div>
			{/if}
		</div>
	{/if}
</div>
