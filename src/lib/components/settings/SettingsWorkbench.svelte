<script lang="ts">
	import SettingsSelect from './SettingsSelect.svelte';
	import { getWorkbenchSettingsStore } from '$stores/context';
	import type { WorktreeStrategy } from '$types/workbench';

	const store = getWorkbenchSettingsStore();

	const strategyOptions = [
		{ value: 'sibling', label: 'Sibling folder' },
		{ value: 'inside', label: 'Inside .worktrees/' }
	];
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
</div>
