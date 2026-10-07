<script lang="ts">
	import { SvelteSet } from 'svelte/reactivity';
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import SearchIcon from '@lucide/svelte/icons/search';
	import StarIcon from '@lucide/svelte/icons/star';
	import XIcon from '@lucide/svelte/icons/x';
	import type { MobileClient } from './client.svelte.ts';
	import type { ReviewFolder } from './project-review.svelte';
	import { projectSections } from '@workbench/control-plane-ui';
	import ProjectRow from './ProjectRow.svelte';

	let {
		client,
		onReview
	}: {
		client: MobileClient;
		onReview: (folder: ReviewFolder, tab: 'history' | 'changes') => void;
	} = $props();

	const store = $derived(client.store!);
	const prefs = $derived(client.projectPrefs);
	let query = $state('');
	/** Kept here so a row stays open when starring or searching remounts it. */
	const expanded = new SvelteSet<string>();
	const searching = $derived(query.trim() !== '');
	const sections = $derived(projectSections(store.projects, prefs.favourites, query));
	const live = $derived.by(() => {
		const counts: Record<string, number> = {};
		for (const chat of client.chats) {
			if (!chat.exited) counts[chat.projectPath] = (counts[chat.projectPath] ?? 0) + 1;
		}
		return counts;
	});
</script>

<section class="flex flex-col gap-3" aria-label="Projects">
	<div
		class="flex h-10 items-center gap-2 rounded-xl border border-wb-hair bg-wb-panel pr-1 pl-3 text-wb-ink-soft focus-within:border-wb-ink-soft"
	>
		<SearchIcon class="size-4 shrink-0" />
		<input
			bind:value={query}
			type="search"
			aria-label="Search projects"
			placeholder="Search {store.projects.length} projects"
			autocapitalize="off"
			autocorrect="off"
			spellcheck={false}
			class="min-w-0 flex-1 bg-transparent text-[13.5px] text-wb-ink placeholder:text-wb-ink-soft focus:outline-none [&::-webkit-search-cancel-button]:hidden"
		/>
		{#if query}
			<button
				type="button"
				class="grid size-8 shrink-0 place-items-center rounded-lg active:bg-wb-panel2"
				aria-label="Clear search"
				onpointerdown={(e) => e.preventDefault()}
				onclick={() => (query = '')}
			>
				<XIcon class="size-4" />
			</button>
		{/if}
	</div>
	{#if store.projects.length === 0}
		<p class="px-1 text-xs text-wb-ink-soft">
			No projects on this server yet. Add one in Workbench on the desktop.
		</p>
	{:else if sections.length === 0}
		<p class="px-1 text-xs text-wb-ink-soft">No projects match “{query}”.</p>
	{:else if !searching && sections[0].kind !== 'favourites'}
		<p class="px-1 text-xs text-wb-ink-soft">Star a project to pin it to the top.</p>
	{/if}
	{#each sections as section (section.key)}
		{@const collapsed = !searching && prefs.collapsed.has(section.key)}
		<div class="flex flex-col gap-2">
			<h2>
				<button
					type="button"
					class="flex h-8 w-full items-center gap-2 px-0.5 text-[11px] font-semibold tracking-[0.08em] text-wb-ink-soft uppercase"
					aria-expanded={!collapsed}
					disabled={searching}
					onclick={() => prefs.toggleSection(section.key)}
				>
					{#if section.kind === 'favourites'}
						<StarIcon class="size-3 fill-wb-warn text-wb-warn" />
					{/if}
					<span class="truncate">{section.title}</span>
					<span class="font-mono tracking-normal text-wb-ink-mute">{section.projects.length}</span>
					{#if !searching}
						<ChevronDownIcon
							class={[
								'ml-auto size-3.5 shrink-0 transition-transform motion-reduce:transition-none',
								collapsed && '-rotate-90'
							]}
						/>
					{/if}
				</button>
			</h2>
			{#if !collapsed}
				{#each section.projects as project (project.path)}
					<ProjectRow
						{client}
						{project}
						favourite={section.kind === 'favourites'}
						live={live[project.path] ?? 0}
						highlight={query}
						open={expanded.has(project.path)}
						onToggle={() => {
							if (!expanded.delete(project.path)) expanded.add(project.path);
						}}
						{onReview}
					/>
				{/each}
			{/if}
		</div>
	{/each}
</section>
