<script lang="ts">
	import { watch } from 'runed';
	import DownloadIcon from '@lucide/svelte/icons/download';
	import PackageIcon from '@lucide/svelte/icons/package';
	import PlayIcon from '@lucide/svelte/icons/play';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import { ScrollArea } from '@workbench/ui/scroll-area';
	import { getGitStore, getWorkspaceStore } from '$stores/context';
	import { effectivePath } from '$lib/utils/path';
	import { PackageScripts, installCommand, runScriptCommand } from './package-scripts.svelte';

	const workspaceStore = getWorkspaceStore();
	const gitStore = getGitStore();
	const scripts = new PackageScripts();

	let workspace = $derived(workspaceStore.activeWorkspace);
	let activePath = $derived(workspace ? effectivePath(workspace) : null);
	let info = $derived(activePath ? scripts.infoByPath[activePath] : undefined);
	let error = $derived(activePath ? scripts.errorByPath[activePath] : undefined);
	let branch = $derived(
		workspace ? (workspace.branch ?? gitStore.branchByProject[workspace.projectPath]) : undefined
	);

	// Read package.json when the active workspace changes (filesystem side effect)
	watch(
		() => activePath,
		(path) => {
			if (path) void scripts.load(path);
		}
	);

	function run(name: string, command: string) {
		if (workspace) workspaceStore.runTaskInWorkspace(workspace.id, { name, command });
	}
</script>

{#if !workspace || !activePath}
	{@render message('Open a project to see its package.json scripts.')}
{:else if error}
	<div class="flex h-full flex-col items-center justify-center gap-2 p-4 text-center">
		<p class="font-mono text-[11px] break-all text-wb-err">{error}</p>
		<button
			type="button"
			class="rounded px-2 py-1 font-mono text-[11px] text-wb-ink-mute hover:bg-wb-panel2 hover:text-wb-ink"
			onclick={() => scripts.load(activePath)}>Retry</button
		>
	</div>
{:else if info === undefined}
	{@render message('Reading package.json…')}
{:else if info === null}
	{@render message('No package.json at the root of this workspace.')}
{:else}
	<div class="flex h-full flex-col">
		<div class="flex shrink-0 flex-col gap-2.5 border-b border-wb-hair p-3">
			<div class="flex items-center gap-2.5">
				<div class="grid size-8 shrink-0 place-items-center rounded-lg bg-wb-accent-soft">
					<PackageIcon class="size-4 text-wb-accent" />
				</div>
				<div class="flex min-w-0 flex-1 flex-col gap-0.5 font-mono">
					<span class="text-[12.5px] font-semibold text-wb-ink"
						>{info.manager}
						{#if info.managerVersion}
							<span class="font-normal text-wb-ink-mute">{info.managerVersion}</span>
						{/if}
					</span>
					<span class="truncate text-[10.5px] text-wb-ink-mute"
						>{info.detectedFrom === 'default' ? 'no lockfile, using npm' : info.detectedFrom}</span
					>
				</div>
				<button
					type="button"
					class="flex h-7 shrink-0 items-center gap-1.5 rounded-md bg-wb-accent px-3 font-mono text-[11.5px] font-semibold text-wb-rail hover:opacity-90"
					title={installCommand(info)}
					onclick={() => run('install', installCommand(info))}
				>
					<DownloadIcon class="size-3" />
					Install
				</button>
			</div>
			<p class="truncate font-mono text-[10.5px] text-wb-ink-mute">
				{workspace.projectName}{branch ? ` / ${branch}` : ''}
			</p>
		</div>

		<div class="flex h-8 shrink-0 items-center px-3 pt-2 pl-[22px]">
			<span class="flex-1 text-[10px] font-semibold tracking-wider text-wb-ink-mute uppercase">
				Scripts <span class="font-mono font-normal">{info.scripts.length}</span>
			</span>
			<button
				type="button"
				class="grid size-[22px] place-items-center rounded text-wb-ink-mute hover:bg-wb-panel2 hover:text-wb-ink"
				aria-label="Reload package.json"
				onclick={() => scripts.load(activePath)}
			>
				<RefreshCwIcon class="size-3" />
			</button>
		</div>

		<ScrollArea class="min-h-0 flex-1">
			{#if info.scripts.length === 0}
				<p class="px-3 py-4 text-center font-mono text-[11px] text-wb-ink-mute">
					package.json has no scripts.
				</p>
			{:else}
				<div class="flex flex-col gap-px px-2 py-1">
					{#each info.scripts as script (script.name)}
						<button
							type="button"
							class="group flex h-[38px] w-full items-center gap-2.5 rounded-md px-2.5 text-left font-mono hover:bg-wb-panel2 focus-visible:bg-wb-panel2 focus-visible:outline-none"
							title={script.command}
							aria-label="Run {script.name}"
							onclick={() => run(script.name, runScriptCommand(info, script))}
						>
							<span class="flex min-w-0 flex-1 flex-col gap-0.5">
								<span class="truncate text-[12px] text-wb-ink">{script.name}</span>
								<span class="truncate text-[10.5px] text-wb-ink-mute">{script.command}</span>
							</span>
							<span
								class="grid size-[22px] shrink-0 place-items-center rounded-[5px] bg-wb-accent text-wb-rail opacity-0 group-hover:opacity-100 group-focus-visible:opacity-100"
							>
								<PlayIcon class="size-2.5 fill-current" />
							</span>
						</button>
					{/each}
				</div>
			{/if}
		</ScrollArea>

		<p
			class="shrink-0 border-t border-wb-hair px-3 py-2.5 font-mono text-[10.5px] leading-relaxed text-wb-ink-mute"
		>
			Each script opens in its own terminal tab in this workspace.
		</p>
	</div>
{/if}

{#snippet message(text: string)}
	<div class="flex h-full items-center justify-center p-4">
		<p class="text-center font-mono text-[11px] text-wb-ink-mute">{text}</p>
	</div>
{/snippet}
