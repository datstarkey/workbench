<script lang="ts">
	import { Button } from '@workbench/ui/button';
	import { Input } from '@workbench/ui/input';
	import type { ProjectConfig } from '@workbench/types';
	import type { ControlPlaneStore } from './control-plane.svelte.ts';

	let {
		store,
		onOpenTerminal
	}: {
		store: ControlPlaneStore;
		/** When provided, render Terminal / Claude affordances that open a live
		 * shell for the given cwd (mobile uses this; desktop has its own local
		 * terminals). `command` runs once the shell starts (e.g. `claude`). */
		onOpenTerminal?: (
			projectPath: string,
			worktreePath: string | undefined,
			name: string,
			command?: string
		) => void;
	} = $props();

	let expanded = $state<string | null>(null);
	let newBranch = $state<Record<string, string>>({});
	let query = $state('');

	// Filter projects by name/path/group, then bucket by `group` (ungrouped last).
	let groups = $derived.by(() => {
		const q = query.trim().toLowerCase();
		const matched = q
			? store.projects.filter(
					(p) =>
						p.name.toLowerCase().includes(q) ||
						p.path.toLowerCase().includes(q) ||
						(p.group ?? '').toLowerCase().includes(q)
				)
			: store.projects;
		const map = new Map<string, ProjectConfig[]>();
		for (const p of matched) {
			const g = p.group ?? '';
			const arr = map.get(g);
			if (arr) arr.push(p);
			else map.set(g, [p]);
		}
		return [...map.entries()].sort((a, b) => {
			if (a[0] === b[0]) return 0;
			if (a[0] === '') return 1;
			if (b[0] === '') return -1;
			return a[0].localeCompare(b[0]);
		});
	});

	function projectName(p: ProjectConfig): string {
		return p.name || p.path.split('/').pop() || p.path;
	}

	async function toggle(path: string) {
		if (expanded === path) {
			expanded = null;
			return;
		}
		expanded = path;
		if (!store.worktrees[path]) await store.loadWorktrees(path);
	}
</script>

<div class="flex h-full flex-col gap-4 overflow-y-auto p-3">
	{#if store.error}
		<p
			class="rounded border border-wb-err/40 bg-wb-err/10 px-2 py-1.5 font-mono text-[11px] text-wb-err"
		>
			{store.error}
		</p>
	{/if}

	<section class="flex flex-col gap-2">
		<h2 class="text-[11px] font-semibold tracking-wider text-wb-ink-soft uppercase">Projects</h2>
		<Input
			bind:value={query}
			placeholder="Search projects…"
			autocapitalize="off"
			autocorrect="off"
			spellcheck={false}
		/>
		{#if store.projects.length === 0}
			<p class="text-xs text-wb-ink-soft">No projects on this server.</p>
		{:else if groups.length === 0}
			<p class="text-xs text-wb-ink-soft">No projects match “{query}”.</p>
		{:else}
			{#each groups as [group, groupProjects] (group)}
				{#if group}
					<h3
						class="mt-1 px-0.5 text-[10px] font-semibold tracking-wider text-wb-ink-mute uppercase"
					>
						{group}
					</h3>
				{/if}
				{#each groupProjects as p (p.path)}
					<div class="overflow-hidden rounded-md border border-wb-hair bg-wb-panel2">
						<div class="flex items-center gap-2 p-2">
							<div class="min-w-0 flex-1">
								<div class="truncate text-sm font-medium text-wb-ink">{projectName(p)}</div>
								<div class="truncate font-mono text-[11px] text-wb-ink-soft">{p.path}</div>
							</div>
							{#if onOpenTerminal}
								<Button
									variant="secondary"
									size="sm"
									onclick={() => onOpenTerminal(p.path, undefined, projectName(p))}
								>
									Term
								</Button>
								<Button
									size="sm"
									onclick={() =>
										onOpenTerminal(p.path, undefined, `${projectName(p)} · claude`, 'claude')}
								>
									Claude
								</Button>
							{/if}
							<Button variant="ghost" size="sm" onclick={() => toggle(p.path)}>
								{expanded === p.path ? '▾' : '▸'}
							</Button>
						</div>

						{#if expanded === p.path}
							<div class="flex flex-col gap-2 border-t border-wb-hair bg-wb-bg/50 p-2">
								{#each store.worktrees[p.path] ?? [] as w (w.path)}
									<div class="flex items-center gap-2">
										<div class="min-w-0 flex-1">
											<div class="truncate text-sm text-wb-ink">{w.branch || '(detached)'}</div>
											<div class="truncate font-mono text-[11px] text-wb-ink-soft">
												{w.path}{w.isMain ? ' · main' : ''}
											</div>
										</div>
										{#if onOpenTerminal}
											<Button
												variant="secondary"
												size="sm"
												onclick={() =>
													onOpenTerminal(
														p.path,
														w.isMain ? undefined : w.path,
														`${projectName(p)}:${w.branch}`
													)}
											>
												Term
											</Button>
											<Button
												size="sm"
												onclick={() =>
													onOpenTerminal(
														p.path,
														w.isMain ? undefined : w.path,
														`${projectName(p)}:${w.branch} · claude`,
														'claude'
													)}
											>
												Claude
											</Button>
										{/if}
									</div>
								{/each}
								<div class="flex gap-2">
									<Input bind:value={newBranch[p.path]} placeholder="new-branch-name" />
									<Button
										variant="secondary"
										size="sm"
										onclick={() => {
											const b = (newBranch[p.path] ?? '').trim();
											if (b) store.createWorktree(p.path, b);
										}}
									>
										Create
									</Button>
								</div>
							</div>
						{/if}
					</div>
				{/each}
			{/each}
		{/if}
	</section>
</div>
