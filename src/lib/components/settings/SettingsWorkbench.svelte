<script lang="ts">
	import { Separator } from '$lib/components/ui/separator';
	import SettingsSelect from './SettingsSelect.svelte';
	import SettingsToggle from './SettingsToggle.svelte';
	import { getWorkbenchSettingsStore } from '$stores/context';
	import { applyClaudeIntegration, applyCodexIntegration } from '$lib/utils/terminal';
	import type { WorktreeStrategy } from '$types/workbench';

	const store = getWorkbenchSettingsStore();

	const strategyOptions = [
		{ value: 'sibling', label: 'Sibling folder' },
		{ value: 'inside', label: 'Inside .worktrees/' }
	];

	async function toggleClaudeHooks(checked: boolean) {
		if (checked) {
			await applyClaudeIntegration();
		}
		await store.setApproval('claude', checked);
	}

	async function toggleCodexConfig(checked: boolean) {
		if (checked) {
			await applyCodexIntegration();
		}
		await store.setApproval('codex', checked);
	}
</script>

<div class="space-y-6">
	<div>
		<h2 class="text-sm font-semibold">Worktrees</h2>
		<p class="mt-1 text-xs text-muted-foreground">Configure how git worktrees are created.</p>
	</div>

	<SettingsSelect
		label="Worktree location"
		description="Where new worktrees are created on disk."
		options={strategyOptions}
		value={store.worktreeStrategy}
		onValueChange={(v) => store.setWorktreeStrategy(v as WorktreeStrategy)}
	/>

	{#if store.worktreeStrategy === 'inside'}
		<p class="text-xs text-muted-foreground">
			Worktrees will be created at <code>&lt;repo&gt;/.worktrees/&lt;branch&gt;</code> and
			<code>.worktrees/</code> will be added to <code>.gitignore</code>.
		</p>
	{:else}
		<p class="text-xs text-muted-foreground">
			Worktrees will be created at <code>&lt;parent&gt;/&lt;repo&gt;-&lt;branch&gt;</code> next to the
			project folder.
		</p>
	{/if}

	<Separator />

	<div>
		<h2 class="text-sm font-semibold">Integrations</h2>
		<p class="mt-1 text-xs text-muted-foreground">
			Control whether Workbench modifies external AI tool configs.
		</p>
	</div>

	<SettingsToggle
		label="Claude hooks"
		description="Register session tracking hooks in ~/.claude/settings.json"
		checked={store.claudeHooksApproved === true}
		onCheckedChange={toggleClaudeHooks}
	/>

	<SettingsToggle
		label="Codex config"
		description="Configure CLAUDE.md fallback and notify script in ~/.codex/config/config.toml"
		checked={store.codexConfigApproved === true}
		onCheckedChange={toggleCodexConfig}
	/>
</div>
