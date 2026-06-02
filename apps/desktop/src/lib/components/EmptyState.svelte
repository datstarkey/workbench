<script lang="ts">
	import FolderIcon from '@lucide/svelte/icons/folder';
	import GithubIcon from '@lucide/svelte/icons/github';
	import BookOpenIcon from '@lucide/svelte/icons/book-open';
	import { open } from '@tauri-apps/plugin-shell';
	import { getProjectManager } from '$stores/context';
	import CloneRepoDialog from '$features/projects/CloneRepoDialog.svelte';

	const projectManager = getProjectManager();

	const DOCS_URL = 'https://github.com/datstarkey/workbench';

	let cloneOpen = $state(false);
</script>

{#snippet bigAction(
	label: string,
	sub: string,
	hint: string,
	primary: boolean,
	onclick: () => void,
	icon: typeof FolderIcon
)}
	{@const Icon = icon}
	<button
		type="button"
		{onclick}
		class="flex items-center gap-3.5 rounded-lg border px-4 py-3 text-left transition-colors {primary
			? 'border-wb-accent bg-wb-panel2 ring-2 ring-wb-accent/20'
			: 'border-wb-hair bg-wb-panel hover:bg-wb-panel2'}"
	>
		<span class="grid size-8 place-items-center rounded-md bg-wb-bg text-wb-accent">
			<Icon size={16} />
		</span>
		<span class="flex-1">
			<span class="block text-[13px] font-medium text-wb-ink">{label}</span>
			<span class="mt-0.5 block text-[11.5px] text-wb-ink-soft">{sub}</span>
		</span>
		{#if hint}
			<span class="font-mono text-[10.5px] text-wb-ink-soft">{hint}</span>
		{/if}
	</button>
{/snippet}

<div class="grid flex-1 place-items-center bg-wb-bg">
	<div class="max-w-[540px] px-8 text-center">
		<div
			class="mx-auto mb-5 grid size-14 place-items-center rounded-xl border border-wb-hair bg-wb-panel font-mono text-2xl font-semibold text-wb-accent"
		>
			W
		</div>
		<h2 class="text-2xl font-semibold text-wb-ink">Welcome to Workbench</h2>
		<p class="mx-auto mt-1.5 text-[13.5px] leading-relaxed text-wb-ink-mute">
			Add a local project folder to get started. Each project gets its own tabbed workspace with
			real shell terminals, Claude Code, Codex, and git worktree support.
		</p>
		<div class="mx-auto mt-6 flex max-w-[380px] flex-col gap-2">
			{@render bigAction(
				'Add project',
				'Pick a folder on your machine',
				'⌘ N',
				true,
				() => projectManager.add(),
				FolderIcon
			)}
			{@render bigAction(
				'Clone from GitHub',
				'Pick from your repositories',
				'⌘ ⇧ C',
				false,
				() => (cloneOpen = true),
				GithubIcon
			)}
			{@render bigAction(
				'Read the docs',
				'Shortcuts, startup commands, integrations',
				'',
				false,
				() => void open(DOCS_URL),
				BookOpenIcon
			)}
		</div>
		<p class="mt-6 font-mono text-[11px] text-wb-ink-soft">
			tip · drag any folder from Finder onto this window
		</p>
	</div>
</div>

<CloneRepoDialog bind:open={cloneOpen} />
