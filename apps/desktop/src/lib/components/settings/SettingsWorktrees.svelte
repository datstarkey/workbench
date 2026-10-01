<script lang="ts">
	import { Input } from '@workbench/ui/input';
	import { getWorkbenchSettingsStore } from '$stores/context';
	import type { WorktreeStartPoint, WorktreeStrategy } from '$types/workbench';
	import SettingsNote from './SettingsNote.svelte';
	import SettingsRow from './SettingsRow.svelte';
	import SettingsSection from './SettingsSection.svelte';
	import SettingsSelect from './SettingsSelect.svelte';
	import SettingsToggle from './SettingsToggle.svelte';

	const store = getWorkbenchSettingsStore();

	const strategyOptions = [
		{ value: 'sibling', label: 'Sibling folder' },
		{ value: 'inside', label: 'Inside .worktrees/' }
	];
	const startPointOptions = [
		{ value: 'auto', label: 'Origin default branch' },
		{ value: 'current', label: 'Current branch' },
		{ value: 'custom', label: 'Custom branch' }
	];
</script>

<SettingsSection title="Location">
	<SettingsSelect
		label="Worktree location"
		description="Where new worktrees are created on disk."
		options={strategyOptions}
		value={store.worktreeStrategy}
		onValueChange={(v) => store.set('worktreeStrategy', v as WorktreeStrategy)}
	>
		<SettingsNote>
			{#if store.worktreeStrategy === 'inside'}
				Created at <code>&lt;repo&gt;/.worktrees/&lt;branch&gt;</code>, and
				<code>.worktrees/</code> is added to <code>.gitignore</code>.
			{:else}
				Created at <code>&lt;parent&gt;/&lt;repo&gt;-&lt;branch&gt;</code>, next to the project
				folder.
			{/if}
		</SettingsNote>
	</SettingsSelect>
</SettingsSection>

<SettingsSection title="New branches">
	<SettingsSelect
		label="Branch from"
		description="Start point for new worktree branches."
		options={startPointOptions}
		value={store.worktreeStartPoint}
		onValueChange={(v) => store.set('worktreeStartPoint', v as WorktreeStartPoint)}
	/>

	{#if store.worktreeStartPoint === 'custom'}
		<SettingsRow
			label="Custom branch"
			description="Branch or ref to start from. Prefix with origin/ for a remote branch."
		>
			{#snippet control()}
				<Input
					class="h-8 w-44 font-mono text-xs"
					placeholder="e.g. develop"
					aria-label="Custom branch"
					value={store.worktreeCustomBranch}
					oninput={(e) => store.set('worktreeCustomBranch', e.currentTarget.value)}
				/>
			{/snippet}
		</SettingsRow>
	{/if}

	<SettingsToggle
		label="Fetch first"
		description="Run git fetch before creating a branch, so it starts from the latest remote state."
		checked={store.worktreeFetchBeforeCreate}
		onCheckedChange={(checked) => store.set('worktreeFetchBeforeCreate', checked)}
	/>
</SettingsSection>
