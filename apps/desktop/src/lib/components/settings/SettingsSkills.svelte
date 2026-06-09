<script lang="ts">
	import { getClaudeSettingsStore } from '$stores/context';
	import SettingsEmptyState from './SettingsEmptyState.svelte';

	const claudeSettingsStore = getClaudeSettingsStore();
</script>

<div class="space-y-4">
	<p class="text-xs text-muted-foreground">
		Skills discovered from <code class="rounded bg-muted px-1">~/.claude/skills/</code>. Each skill
		is a directory containing a SKILL.md file.
	</p>

	{#if claudeSettingsStore.skills.length === 0}
		<SettingsEmptyState
			title="No skills found."
			subtitle="Add skill directories to ~/.claude/skills/"
		/>
	{:else}
		<div class="space-y-2">
			{#each claudeSettingsStore.skills as skill (skill.dirName)}
				<div class="rounded-md border border-border/60 px-3 py-2">
					<div class="text-sm font-medium">{skill.name}</div>
					{#if skill.description}
						<p class="mt-0.5 line-clamp-2 text-xs text-muted-foreground">
							{skill.description}
						</p>
					{/if}
				</div>
			{/each}
		</div>
	{/if}
</div>
