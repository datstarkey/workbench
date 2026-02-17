<script lang="ts">
	import DownloadIcon from '@lucide/svelte/icons/download';
	import FolderIcon from '@lucide/svelte/icons/folder';
	import LoaderIcon from '@lucide/svelte/icons/loader';
	import LockIcon from '@lucide/svelte/icons/lock';
	import GitForkIcon from '@lucide/svelte/icons/git-fork';
	import SearchIcon from '@lucide/svelte/icons/search';
	import * as Dialog from '$lib/components/ui/dialog';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import { Badge } from '$lib/components/ui/badge';
	import { Separator } from '$lib/components/ui/separator';
	import { ScrollArea } from '$lib/components/ui/scroll-area';
	import { invoke } from '@tauri-apps/api/core';
	import { selectFolder } from '$lib/utils/dialog';
	import { getProjectStore, getWorkbenchSettingsStore } from '$stores/context';
	import type { GitHubRepo } from '$types/workbench';

	let {
		open = $bindable(false)
	}: {
		open?: boolean;
	} = $props();

	const projectStore = getProjectStore();
	const settingsStore = getWorkbenchSettingsStore();

	let repos: GitHubRepo[] = $state([]);
	let loadingRepos = $state(false);
	let reposError: string | null = $state(null);
	let searchQuery = $state('');
	let selectedRepo: GitHubRepo | null = $state(null);
	let manualInput = $state('');
	let cloneDir = $state('');
	let cloning = $state(false);
	let cloneError: string | null = $state(null);

	let filteredRepos = $derived.by(() => {
		const q = searchQuery.toLowerCase();
		if (!q) return repos;
		return repos.filter(
			(r) =>
				r.nameWithOwner.toLowerCase().includes(q) ||
				(r.description ?? '').toLowerCase().includes(q)
		);
	});

	let resolvedUrl = $derived.by(() => {
		if (selectedRepo) return selectedRepo.url;
		const m = manualInput.trim();
		if (!m) return '';
		if (m.includes('://') || m.startsWith('git@')) return m;
		return `https://github.com/${m}`;
	});

	let repoName = $derived.by(() => {
		if (selectedRepo) return selectedRepo.name;
		const m = manualInput.trim();
		if (!m) return '';
		const parts = m.replace(/\.git$/, '').split('/');
		return parts[parts.length - 1] ?? '';
	});

	let resolvedDestPath = $derived.by(() => {
		if (!cloneDir || !repoName) return '';
		const sep = cloneDir.includes('\\') ? '\\' : '/';
		return `${cloneDir}${sep}${repoName}`;
	});

	function resetState() {
		selectedRepo = null;
		manualInput = '';
		searchQuery = '';
		cloneError = null;
		cloneDir = settingsStore.cloneBaseDir ?? '';
		repos = [];
		reposError = null;
	}

	// $effect is appropriate here — network request is an external side effect.
	// onOpenChange is only fired by user interactions, not programmatic open changes.
	$effect(() => {
		if (open) {
			resetState();
			loadRepos();
		}
	});

	async function loadRepos() {
		loadingRepos = true;
		reposError = null;
		try {
			repos = await invoke<GitHubRepo[]>('github_list_repos');
		} catch (e) {
			reposError = String(e);
		} finally {
			loadingRepos = false;
		}
	}

	async function pickDir() {
		const dir = await selectFolder(cloneDir || undefined, 'Select Destination Directory');
		if (dir !== null) cloneDir = dir;
	}

	async function handleClone() {
		if (!resolvedUrl || !resolvedDestPath) return;
		cloning = true;
		cloneError = null;
		try {
			await invoke('clone_repo', { url: resolvedUrl, destPath: resolvedDestPath });
			const parts = resolvedDestPath.split('/');
			const name = repoName || parts[parts.length - 1] || 'Cloned Repo';
			await projectStore.add({ name, path: resolvedDestPath });
			projectStore.openProject(resolvedDestPath);
			open = false;
		} catch (e) {
			cloneError = String(e);
		} finally {
			cloning = false;
		}
	}
</script>

<Dialog.Root bind:open>
	<Dialog.Content class="flex max-h-[90vh] max-w-2xl flex-col overflow-hidden">
		<Dialog.Header class="shrink-0">
			<Dialog.Title class="flex items-center gap-2">
				<DownloadIcon class="size-4" />
				Clone Repository
			</Dialog.Title>
			<Dialog.Description>
				Choose a repository from your account or enter a URL to clone.
			</Dialog.Description>
		</Dialog.Header>

		<div class="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto">
			<!-- Repo browser -->
			<div class="flex flex-col gap-2">
				<Label class="text-xs text-muted-foreground">Your repositories</Label>
				<div class="relative">
					<SearchIcon class="absolute top-2 left-2.5 size-3.5 text-muted-foreground" />
					<Input
						class="h-8 pl-8 text-xs"
						placeholder="Search repositories..."
						bind:value={searchQuery}
					/>
				</div>

				{#if loadingRepos}
					<div class="flex items-center justify-center py-6">
						<LoaderIcon class="size-4 animate-spin text-muted-foreground" />
					</div>
				{:else if reposError}
					<div class="rounded-md border border-destructive/40 bg-destructive/10 p-3 space-y-2">
						<p class="text-xs font-medium text-destructive">Failed to load repositories</p>
						<p class="text-[11px] text-muted-foreground font-mono break-all">{reposError}</p>
						<Button variant="outline" size="sm" class="h-7 text-xs" onclick={loadRepos}>
							Retry
						</Button>
					</div>
				{:else}
					<ScrollArea class="h-48 rounded-md border">
						<div class="divide-y">
							{#each filteredRepos as repo (repo.nameWithOwner)}
								<button
									class="flex w-full items-center gap-2 px-3 py-2 text-left text-xs transition-colors hover:bg-muted/50 {selectedRepo?.nameWithOwner === repo.nameWithOwner ? 'bg-accent' : ''}"
									type="button"
									onclick={() => {
										selectedRepo = repo;
										manualInput = '';
									}}
								>
									<div class="min-w-0 flex-1">
										<span class="font-medium">{repo.nameWithOwner}</span>
										{#if repo.description}
											<span class="ml-1.5 truncate text-muted-foreground"
												>{repo.description}</span
											>
										{/if}
									</div>
									<div class="flex shrink-0 items-center gap-1">
										{#if repo.isPrivate}
											<Badge variant="outline" class="h-4 px-1 text-[9px]">Private</Badge>
										{/if}
										{#if repo.isFork}
											<GitForkIcon class="size-3 text-muted-foreground/60" />
										{/if}
									</div>
								</button>
							{:else}
								<p class="px-3 py-4 text-center text-xs text-muted-foreground">
									{searchQuery ? 'No matching repositories' : 'No repositories found'}
								</p>
							{/each}
						</div>
					</ScrollArea>
				{/if}
			</div>

			<div class="flex items-center gap-3">
				<Separator class="flex-1" />
				<span class="shrink-0 text-[10px] text-muted-foreground uppercase">or enter URL / owner/repo</span>
				<Separator class="flex-1" />
			</div>

			<Input
				class="h-8 text-xs"
				placeholder="https://github.com/owner/repo  or  owner/repo"
				bind:value={manualInput}
				oninput={() => (selectedRepo = null)}
			/>

			<!-- Destination row -->
			<div class="flex flex-col gap-1.5">
				<Label class="text-xs text-muted-foreground">Destination</Label>
				<div class="flex items-center gap-2">
					<Input
						class="h-8 flex-1 font-mono text-xs text-muted-foreground"
						value={resolvedDestPath || '(choose a directory)'}
						readonly
					/>
					<Button variant="outline" size="sm" class="h-8 shrink-0 gap-1.5" onclick={pickDir}>
						<FolderIcon class="size-3" />
						Browse
					</Button>
				</div>
			</div>

			{#if cloneError}
				<p class="text-xs text-destructive">{cloneError}</p>
			{/if}
		</div>

		<Dialog.Footer class="shrink-0">
			<Button variant="ghost" size="sm" onclick={() => (open = false)}>Cancel</Button>
			<Button
				size="sm"
				class="gap-1.5"
				onclick={handleClone}
				disabled={!resolvedUrl || !resolvedDestPath || cloning}
			>
				{#if cloning}
					<LoaderIcon class="size-3 animate-spin" />
					Cloning...
				{:else}
					<DownloadIcon class="size-3" />
					Clone
				{/if}
			</Button>
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>
