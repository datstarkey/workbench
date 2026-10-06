<script lang="ts">
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import ExternalLinkIcon from '@lucide/svelte/icons/external-link';
	import FileDiffIcon from '@lucide/svelte/icons/file-diff';
	import GitBranchIcon from '@lucide/svelte/icons/git-branch';
	import HistoryIcon from '@lucide/svelte/icons/history';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import StarIcon from '@lucide/svelte/icons/star';
	import type { ProjectConfig } from '@workbench/types';
	import { openExternal, type MobileClient } from './client.svelte.ts';
	import { baseName, repoLabel, tildePath } from './home-format.ts';
	import type { ReviewFolder } from './project-review.svelte';
	import { matchParts } from './project-sections.ts';
	import StartButtons from './StartButtons.svelte';

	let {
		client,
		project,
		favourite,
		live,
		highlight = '',
		open,
		onToggle,
		onReview
	}: {
		client: MobileClient;
		project: ProjectConfig;
		favourite: boolean;
		/** Chats running in this project. */
		live: number;
		/** Search text to mark in the project name. */
		highlight?: string;
		open: boolean;
		onToggle: () => void;
		onReview: (folder: ReviewFolder, tab: 'history' | 'changes') => void;
	} = $props();

	const store = $derived(client.store!);
	const name = $derived(project.name || baseName(project.path));
	const worktrees = $derived((store.worktrees[project.path] ?? []).filter((w) => !w.isMain));
	const githubUrl = $derived(store.githubUrls[project.path]);
	let newBranch = $state('');

	const chip =
		'flex h-8 min-w-0 items-center gap-1.5 rounded-lg border border-wb-hair bg-wb-panel px-2.5 text-[12px] text-wb-ink-mute active:bg-wb-panel2';

	async function toggle() {
		const opening = !open;
		onToggle();
		if (!opening) return;
		void store.loadGithubUrl(project.path);
		if (!store.worktrees[project.path]) await store.loadWorktrees(project.path);
	}

	function createWorktree(e: SubmitEvent) {
		e.preventDefault();
		const branch = newBranch.trim();
		if (!branch) return;
		newBranch = '';
		void store.createWorktree(project.path, branch);
	}
</script>

{#snippet reviewChips(worktreePath: string | undefined, folderName: string)}
	{@const folder = { projectPath: project.path, worktreePath, name: folderName }}
	<div class="flex flex-wrap gap-1.5">
		<button type="button" class={chip} onclick={() => onReview(folder, 'history')}>
			<HistoryIcon class="size-3.5 shrink-0" />History
		</button>
		<button type="button" class={chip} onclick={() => onReview(folder, 'changes')}>
			<FileDiffIcon class="size-3.5 shrink-0" />Changes
		</button>
		{#if !worktreePath && githubUrl}
			<button type="button" class={chip} onclick={() => openExternal(githubUrl)}>
				<ExternalLinkIcon class="size-3.5 shrink-0" />
				<span class="truncate font-mono"
					><span class="sr-only">Open on GitHub: </span>{repoLabel(githubUrl)}</span
				>
			</button>
		{/if}
	</div>
{/snippet}

<div class="overflow-hidden rounded-xl border border-wb-hair-soft bg-wb-panel">
	<div class="flex items-center gap-1 py-1.5 pr-1.5 pl-1">
		<button
			type="button"
			class="grid size-9 shrink-0 place-items-center rounded-lg active:bg-wb-panel2"
			aria-label="Favourite {name}"
			aria-pressed={favourite}
			onclick={() => client.projectPrefs.toggleFavourite(project.path)}
		>
			<StarIcon class={['size-4', favourite ? 'fill-wb-warn text-wb-warn' : 'text-wb-ink-soft']} />
		</button>
		<button
			type="button"
			class="flex min-w-0 flex-1 items-center gap-2 self-stretch text-left"
			aria-expanded={open}
			onclick={toggle}
		>
			<span class="flex min-w-0 flex-1 flex-col">
				<span class="truncate text-[14px] font-semibold"
					>{#each matchParts(name, highlight) as part, i (i)}{#if part.match}<mark
								class="rounded-sm bg-wb-accent/25 text-wb-ink">{part.text}</mark
							>{:else}{part.text}{/if}{/each}</span
				>
				<span class="truncate font-mono text-[11px] text-wb-ink-soft"
					>{tildePath(project.path)}</span
				>
			</span>
			{#if live > 0}
				<span
					class="shrink-0 rounded-full bg-wb-ok/15 px-2 py-0.5 font-mono text-[10.5px] text-wb-ok"
					>{live} live</span
				>
			{/if}
			<ChevronDownIcon
				class={[
					'size-4 shrink-0 text-wb-ink-soft transition-transform motion-reduce:transition-none',
					open && 'rotate-180'
				]}
			/>
		</button>
		{#if !open}
			<button
				type="button"
				class="h-9 shrink-0 rounded-lg bg-wb-accent px-3 text-[12.5px] font-semibold text-wb-accent-ink active:brightness-90"
				aria-label="Start Claude in {name}"
				onclick={() => client.startClaude(project.path, undefined, name)}
			>
				Claude
			</button>
		{/if}
	</div>
	{#if open}
		<div class="flex flex-col gap-3 border-t border-wb-hair-soft bg-wb-rail/50 p-3">
			<StartButtons {client} projectPath={project.path} worktreePath={undefined} {name} wide />
			{@render reviewChips(undefined, name)}
			{#if worktrees.length > 0}
				<div class="flex flex-col gap-2">
					<h3
						class="flex items-center gap-2 text-[11px] font-semibold tracking-[0.08em] text-wb-ink-soft uppercase"
					>
						Worktrees
						<span class="font-mono tracking-normal text-wb-ink-mute">{worktrees.length}</span>
					</h3>
					{#each worktrees as w (w.path)}
						{@const label = `${name} · ${w.branch || baseName(w.path)}`}
						<div
							class="flex flex-col gap-2 rounded-lg border border-wb-hair-soft bg-wb-panel p-2.5"
						>
							<div class="flex items-center gap-2">
								<GitBranchIcon class="size-3.5 shrink-0 text-wb-ink-soft" />
								<span class="flex min-w-0 flex-1 flex-col">
									<span class="truncate font-mono text-[12.5px]">{w.branch || '(detached)'}</span>
									<span class="truncate font-mono text-[10.5px] text-wb-ink-soft"
										>{baseName(w.path)}</span
									>
								</span>
								<StartButtons
									{client}
									projectPath={project.path}
									worktreePath={w.path}
									name={label}
								/>
							</div>
							{@render reviewChips(w.path, label)}
						</div>
					{/each}
				</div>
			{/if}
			<form class="flex gap-2" onsubmit={createWorktree}>
				<input
					bind:value={newBranch}
					placeholder="new-branch-name"
					aria-label="New worktree branch for {name}"
					autocapitalize="off"
					autocorrect="off"
					spellcheck={false}
					class="h-9 min-w-0 flex-1 rounded-lg border border-wb-hair bg-wb-panel px-2.5 font-mono text-[12px] text-wb-ink placeholder:text-wb-ink-soft focus:border-wb-ink-soft focus:outline-none"
				/>
				<button
					type="submit"
					disabled={!newBranch.trim()}
					class="flex h-9 shrink-0 items-center gap-1.5 rounded-lg border border-wb-hair bg-wb-panel2 px-3 text-[12px] font-medium text-wb-ink-mute active:bg-wb-panel disabled:opacity-50"
				>
					<PlusIcon class="size-3.5" />Worktree
				</button>
			</form>
		</div>
	{/if}
</div>
