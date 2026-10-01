<script lang="ts">
	import { getClaudeSettingsStore } from '$stores/context';
	import SettingsEmptyState from './SettingsEmptyState.svelte';
	import SettingsSection from './SettingsSection.svelte';

	const claudeSettingsStore = getClaudeSettingsStore();
</script>

{#if claudeSettingsStore.skills.length === 0}
	<SettingsEmptyState title="No skills found." subtitle="Add skill folders to ~/.claude/skills/" />
{:else}
	<SettingsSection>
		{#each claudeSettingsStore.skills as skill (skill.dirName)}
			<div class="px-3.5 py-3">
				<div class="text-[13px] text-wb-ink">{skill.name}</div>
				{#if skill.description}
					<p class="mt-0.5 line-clamp-2 text-xs leading-relaxed text-wb-ink-mute">
						{skill.description}
					</p>
				{/if}
			</div>
		{/each}
	</SettingsSection>
{/if}
