<script lang="ts">
	import { getWorkbenchSettingsStore } from '$stores/context';
	import { applyCodexIntegration } from '$lib/utils/terminal';
	import type { CodexApprovalPolicy, CodexSandboxMode } from '$types/workbench';
	import SettingsNote from './SettingsNote.svelte';
	import SettingsSection from './SettingsSection.svelte';
	import SettingsSelect from './SettingsSelect.svelte';
	import SettingsToggle from './SettingsToggle.svelte';

	const store = getWorkbenchSettingsStore();

	const approvalOptions = [
		{ value: 'default', label: 'Codex default' },
		{ value: 'on-request', label: 'On request' },
		{ value: 'never', label: 'Never' }
	];
	const sandboxOptions = [
		{ value: 'default', label: 'Codex default' },
		{ value: 'read-only', label: 'Read-only' },
		{ value: 'workspace-write', label: 'Workspace write' },
		{ value: 'danger-full-access', label: 'Full access' }
	];

	async function toggleNotifyBridge(checked: boolean) {
		if (checked) await applyCodexIntegration();
		await store.setApproval('codex', checked);
	}
</script>

{#snippet approvalDescription()}
	Sets <code>approval_policy</code>. On request lets Codex decide when to ask; Never sends failures
	back to the model instead.
{/snippet}

{#snippet sandboxDescription()}
	Sets <code>sandbox_mode</code>: what commands Codex runs may read and write.
{/snippet}

{#snippet notifyDescription()}
	Activity status through a notify script, and <code>CLAUDE.md</code> as a project-doc fallback, in
	<code>~/.codex/config.toml</code>.
{/snippet}

<SettingsSection title="Permissions">
	<SettingsSelect
		label="Approval policy"
		description={approvalDescription}
		options={approvalOptions}
		value={store.codexApprovalPolicy}
		onValueChange={(v) => store.set('codexApprovalPolicy', v as CodexApprovalPolicy)}
	/>

	<SettingsSelect
		label="Sandbox mode"
		description={sandboxDescription}
		options={sandboxOptions}
		value={store.codexSandboxMode}
		onValueChange={(v) => store.set('codexSandboxMode', v as CodexSandboxMode)}
	>
		{#if store.codexSandboxMode === 'danger-full-access'}
			<SettingsNote tone="warn">
				Commands run unsandboxed, with full disk and network access. Only use this inside a
				container or VM.
			</SettingsNote>
		{:else}
			<SettingsNote>
				The Workbench sandbox runtime only wraps Claude. Codex uses its own.
			</SettingsNote>
		{/if}
	</SettingsSelect>
</SettingsSection>

<SettingsSection title="Integration">
	<SettingsToggle
		label="Notify bridge"
		description={notifyDescription}
		checked={store.codexConfigApproved === true}
		onCheckedChange={toggleNotifyBridge}
	/>
</SettingsSection>
