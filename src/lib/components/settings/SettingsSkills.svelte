<script lang="ts">
	import { claudeSettingsStore } from '$stores/claude-settings.svelte';
</script>

<div class="space-y-4">
	<p class="text-xs text-muted-foreground">
		Skills discovered from <code class="rounded bg-muted px-1">~/.claude/skills/</code>. Each skill
		is a directory containing a SKILL.md file.
	</p>

	{#if claudeSettingsStore.skills.length === 0}
		<div class="rounded-md border border-dashed border-border/60 py-8 text-center">
			<p class="text-sm text-muted-foreground">No skills found.</p>
			<p class="mt-1 text-xs text-muted-foreground/60">
				Add skill directories to ~/.claude/skills/
			</p>
		</div>
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
