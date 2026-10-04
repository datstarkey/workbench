<script lang="ts">
	import { getWorkbenchSettingsStore } from '$stores/context';
	import { IS_WINDOWS } from '$lib/utils/claude';
	import type { ClaudePermissionMode, PaneView } from '$types/workbench';
	import EditableStringList from './EditableStringList.svelte';
	import SettingsNote from './SettingsNote.svelte';
	import SettingsRow from './SettingsRow.svelte';
	import SettingsSection from './SettingsSection.svelte';
	import SettingsSelect from './SettingsSelect.svelte';
	import SettingsToggle from './SettingsToggle.svelte';

	const store = getWorkbenchSettingsStore();

	// Bypass is only confined when the wrapper will actually be applied.
	const sandboxRuntimeActive = $derived(store.sandboxSettingsPath !== undefined);

	const viewOptions = [
		{ value: 'terminal', label: 'Terminal' },
		{ value: 'chat', label: 'Chat' }
	];
	const permissionModeOptions = [
		{ value: 'default', label: 'Default' },
		{ value: 'acceptEdits', label: 'Accept edits' },
		{ value: 'plan', label: 'Plan' },
		{ value: 'dontAsk', label: "Don't ask" },
		{ value: 'auto', label: 'Auto' },
		{ value: 'bypassPermissions', label: 'Bypass permissions' }
	];

	const access = [
		{
			title: 'Writable',
			body: 'Project folders, /tmp and Claude’s own session state under ~/.claude'
		},
		{
			title: 'Read-only',
			body: 'Everything else, including Claude’s and the project’s hooks, plugins, skills, agents, commands and MCP servers'
		},
		{ title: 'Unreadable', body: '~/.ssh ~/.aws ~/.gnupg ~/.netrc ~/.config/gh ~/.docker ~/.kube' }
	];
</script>

{#snippet permissionModeDescription()}
	Passed as <code>--permission-mode</code>. Wins over <code>permissions.defaultMode</code> in settings.json.
{/snippet}

<SettingsSection title="New sessions">
	<SettingsSelect
		label="Open new Claude tabs as"
		description="Chat shows messages with approval buttons. Any tab can switch between the two."
		options={viewOptions}
		value={store.defaultClaudeView}
		onValueChange={(v) => store.set('defaultClaudeView', v as PaneView)}
	>
		{#if store.defaultClaudeView === 'chat' && sandboxRuntimeActive}
			<SettingsNote>
				Chat doesn't run inside the sandbox runtime yet, so new tabs open as terminals while it's
				on.
			</SettingsNote>
		{/if}
	</SettingsSelect>

	<SettingsSelect
		label="Permission mode"
		description={permissionModeDescription}
		options={permissionModeOptions}
		value={store.claudePermissionMode}
		onValueChange={(v) => store.set('claudePermissionMode', v as ClaudePermissionMode)}
	>
		{#if store.claudePermissionMode === 'bypassPermissions'}
			{#if sandboxRuntimeActive}
				<SettingsNote>
					Bypass skips every permission check, but the sandbox runtime confines the session to this
					project.
				</SettingsNote>
			{:else}
				<SettingsNote tone="warn">
					Bypass skips every permission check. The Bash sandbox only covers shell commands, not file
					tools, MCP servers or hooks. Prefer Auto unless Claude runs inside a container or the
					sandbox runtime.
				</SettingsNote>
			{/if}
		{/if}
	</SettingsSelect>
</SettingsSection>

{#if !IS_WINDOWS}
	<SettingsSection title="Sandbox runtime">
		<SettingsToggle
			label="Run Claude in sandbox runtime"
			description="Confines file tools, MCP servers and hooks to the project, not just Bash. Needs Node (npx); on Linux also bubblewrap, socat and ripgrep. The first launch downloads @anthropic-ai/sandbox-runtime."
			checked={store.sandboxRuntimeEnabled}
			onCheckedChange={(v) => store.set('sandboxRuntimeEnabled', v)}
		/>

		{#if store.sandboxRuntimeEnabled}
			<SettingsRow
				label="Allowed domains"
				description="Network access is denied by default. Anthropic's hosts are pre-filled."
				stack
			>
				{#snippet control()}
					<EditableStringList
						items={store.sandboxAllowedDomains}
						onAdd={(v) => store.addSandboxAllowedDomain(v)}
						onRemove={(v) => store.removeSandboxAllowedDomain(v)}
						placeholder="e.g. github.com"
					/>
				{/snippet}
			</SettingsRow>

			<SettingsRow label="Access" stack>
				{#snippet control()}
					<div class="grid grid-cols-3 gap-2.5">
						{#each access as col (col.title)}
							<div class="rounded-md border border-wb-hair bg-wb-bg px-2.5 py-2">
								<h3 class="mb-1 text-[11px] font-semibold text-wb-ink-mute">{col.title}</h3>
								<p class="text-[11.5px] leading-relaxed text-wb-ink">{col.body}</p>
							</div>
						{/each}
					</div>
				{/snippet}
			</SettingsRow>
		{/if}
	</SettingsSection>
{/if}
