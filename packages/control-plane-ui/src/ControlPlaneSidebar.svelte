<script lang="ts">
	import { Button } from '@workbench/ui/button';
	import { Input } from '@workbench/ui/input';
	import type { ProjectConfig } from '@workbench/types';
	import type { ControlPlaneStore } from './control-plane.svelte.ts';

	let { store }: { store: ControlPlaneStore } = $props();

	let expanded = $state<string | null>(null);
	let newBranch = $state<Record<string, string>>({});

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

	function statusLabel(s: { status: unknown }): string {
		if (s.status === 'starting') return 'starting';
		if (s.status === 'running') return 'running';
		return 'exited';
	}
</script>

<div class="flex h-full flex-col gap-4 overflow-y-auto p-3">
	{#if store.error}
		<p class="text-xs text-red-400">{store.error}</p>
	{/if}

	<section class="flex flex-col gap-2">
		<div class="flex items-center justify-between">
			<h2 class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">Sessions</h2>
			<Button variant="ghost" size="sm" onclick={() => store.refreshSessions()}>Refresh</Button>
		</div>
		{#if store.sessions.length === 0}
			<p class="text-xs text-muted-foreground">No running sessions.</p>
		{:else}
			{#each store.sessions as s (s.id)}
				<div class="flex items-center gap-2 rounded-md border border-border p-2">
					<div class="min-w-0 flex-1">
						<div class="truncate text-sm font-medium">{s.name || s.id.slice(0, 8)}</div>
						<div class="truncate text-xs text-muted-foreground">
							{statusLabel(s)}
							{#if s.sessionUrl}
								· <a class="underline" href={s.sessionUrl} target="_blank" rel="noreferrer">open</a>
							{/if}
						</div>
					</div>
					<Button variant="ghost" size="sm" onclick={() => store.killSession(s.id)}>Kill</Button>
				</div>
			{/each}
		{/if}
	</section>

	<section class="flex flex-col gap-2">
		<h2 class="text-xs font-semibold tracking-wide text-muted-foreground uppercase">Projects</h2>
		{#if store.projects.length === 0}
			<p class="text-xs text-muted-foreground">No projects on this server.</p>
		{:else}
			{#each store.projects as p (p.path)}
				<div class="rounded-md border border-border">
					<div class="flex items-center gap-2 p-2">
						<div class="min-w-0 flex-1">
							<div class="truncate text-sm font-medium">{projectName(p)}</div>
							<div class="truncate text-xs text-muted-foreground">{p.path}</div>
						</div>
						<Button size="sm" onclick={() => store.spawn(p.path, undefined, projectName(p))}>
							Spawn
						</Button>
						<Button variant="ghost" size="sm" onclick={() => toggle(p.path)}>
							{expanded === p.path ? '▾' : '▸'}
						</Button>
					</div>

					{#if expanded === p.path}
						<div class="flex flex-col gap-2 border-t border-border bg-muted/30 p-2">
							{#each store.worktrees[p.path] ?? [] as w (w.path)}
								<div class="flex items-center gap-2">
									<div class="min-w-0 flex-1">
										<div class="truncate text-sm">{w.branch || '(detached)'}</div>
										<div class="truncate text-xs text-muted-foreground">
											{w.path}{w.isMain ? ' · main' : ''}
										</div>
									</div>
									<Button
										size="sm"
										onclick={() =>
											store.spawn(
												p.path,
												w.isMain ? undefined : w.path,
												`${projectName(p)}:${w.branch}`
											)}
									>
										Spawn
									</Button>
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
		{/if}
	</section>
</div>
