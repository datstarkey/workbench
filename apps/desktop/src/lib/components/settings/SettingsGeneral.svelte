<script lang="ts">
	import { onMount } from 'svelte';
	import { Button } from '@workbench/ui/button';
	import { AutostartStore } from '$stores/autostart.svelte';
	import { getWorkbenchSettingsStore } from '$stores/context';
	import { selectFolder } from '$lib/utils/dialog';
	import type { AccentColor } from '$types/workbench';
	import SettingsRow from './SettingsRow.svelte';
	import SettingsSection from './SettingsSection.svelte';
	import SettingsToggle from './SettingsToggle.svelte';

	const store = getWorkbenchSettingsStore();
	const autostart = new AutostartStore();
	onMount(() => {
		void autostart.load();
	});

	// Swatch values mirror the --wb-accent token for each [data-accent] preset in theme.css.
	const accentOptions: { value: AccentColor; label: string; swatch: string }[] = [
		{ value: 'violet', label: 'Violet', swatch: 'oklch(0.68 0.16 280)' },
		{ value: 'tideline', label: 'Tideline', swatch: '#7aa5ff' },
		{ value: 'ember', label: 'Ember', swatch: '#d18a6a' },
		{ value: 'moss', label: 'Moss', swatch: '#5fc78b' },
		{ value: 'iris', label: 'Iris', swatch: '#b783e8' }
	];

	async function pickCloneDir() {
		const dir = await selectFolder(
			store.cloneBaseDir ?? undefined,
			'Select Default Clone Directory'
		);
		if (dir !== null) store.set('cloneBaseDir', dir);
	}
</script>

{#if autostart.supported}
	<SettingsSection title="Startup">
		<SettingsToggle
			label="Start on startup"
			description="Launch Workbench when you sign in to your computer. Changes apply immediately."
			checked={autostart.enabled ?? false}
			disabled={autostart.disabled}
			onCheckedChange={(v) => {
				void autostart.setEnabled(v);
			}}
		/>
		{#if autostart.error}
			<div class="flex items-center justify-between gap-3">
				<p class="text-xs text-destructive" role="alert">{autostart.error}</p>
				<Button
					variant="outline"
					size="sm"
					disabled={autostart.busy}
					onclick={() => autostart.load()}>Retry</Button
				>
			</div>
		{/if}
	</SettingsSection>
{/if}

<SettingsSection title="Appearance">
	<SettingsRow
		label="Accent color"
		description="Buttons, the active worktree and tab highlights. Claude and Codex keep their own session colors."
		stack
	>
		{#snippet control()}
			<div class="flex flex-wrap gap-1.5">
				{#each accentOptions as option (option.value)}
					{@const active = store.accentColor === option.value}
					<button
						type="button"
						aria-pressed={active}
						class={[
							'flex h-7.5 items-center gap-2 rounded-md border pr-3 pl-2 text-xs transition-colors',
							active
								? 'border-wb-accent bg-wb-panel2 text-wb-ink'
								: 'border-wb-hair text-wb-ink-mute hover:text-wb-ink'
						]}
						onclick={() => store.set('accentColor', option.value)}
					>
						<span class="size-3.5 rounded-full" style:background={option.swatch}></span>
						{option.label}
					</button>
				{/each}
			</div>
		{/snippet}
	</SettingsRow>
</SettingsSection>

<SettingsSection title="Sidebar">
	<SettingsToggle
		label="Git tab"
		description="Staging, commits, branches and stashes in the right sidebar."
		checked={store.gitSidebarEnabled}
		onCheckedChange={(v) => store.set('gitSidebarEnabled', v)}
	/>
</SettingsSection>

<SettingsSection title="Projects">
	<SettingsRow
		label="Default clone directory"
		description={store.cloneBaseDir ?? "Not set, so you're asked each time."}
	>
		{#snippet control()}
			<Button variant="outline" size="sm" onclick={pickCloneDir}>Browse…</Button>
		{/snippet}
	</SettingsRow>
</SettingsSection>
