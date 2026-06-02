<script lang="ts">
	import * as Dialog from '$lib/components/ui/dialog';
	import { Input } from '$lib/components/ui/input';
	import { Label } from '$lib/components/ui/label';
	import * as Select from '$lib/components/ui/select';
	import { Switch } from '$lib/components/ui/switch';
	import type { BranchInfo, WorktreeCopyOptions } from '$types/workbench';
	import { baseName } from '$lib/utils/path';
	import { getWorkbenchSettingsStore } from '$stores/context';

	const workbenchSettings = getWorkbenchSettingsStore();

	let {
		open = $bindable(),
		branches,
		projectPath,
		error,
		suggestedBranch = '',
		onSave
	}: {
		open: boolean;
		branches: BranchInfo[];
		projectPath: string;
		error: string;
		suggestedBranch?: string;
		onSave: (
			branch: string,
			newBranch: boolean,
			path: string,
			copyOptions: WorktreeCopyOptions
		) => void;
	} = $props();

	let mode: 'new' | 'existing' = $state('new');
	let newBranchName: string = $state('');
	let selectedBranch: string = $state('');
	let copyAiConfig = $state(true);
	let copyEnvFiles = $state(true);

	let branchName = $derived(mode === 'new' ? newBranchName : selectedBranch);

	let parentDir = $derived.by(() => {
		const trimmed = projectPath.replace(/[\\/]$/, '');
		const lastSep = Math.max(trimmed.lastIndexOf('/'), trimmed.lastIndexOf('\\'));
		return lastSep >= 0 ? trimmed.substring(0, lastSep) : trimmed;
	});
	let projectDirName = $derived(baseName(projectPath));
	let sep = $derived(projectPath.includes('\\') ? '\\' : '/');
	let worktreePath = $derived.by(() => {
		if (!branchName) return '';
		if (workbenchSettings.worktreeStrategy === 'inside') {
			return `${projectPath}${sep}.worktrees${sep}${branchName}`;
		}
		return `${parentDir}${sep}${projectDirName}-${branchName}`;
	});

	let localBranches = $derived(branches.filter((b) => !b.isRemote && !b.isCurrent));

	function handleSave() {
		if (!branchName) return;
		onSave(branchName, mode === 'new', worktreePath, {
			aiConfig: copyAiConfig,
			envFiles: copyEnvFiles
		});
	}

	function resetState() {
		mode = 'new';
		newBranchName = suggestedBranch;
		selectedBranch = '';
		copyAiConfig = true;
		copyEnvFiles = true;
	}
</script>

<Dialog.Root
	bind:open
	onOpenChange={(isOpen) => {
		if (isOpen) {
			newBranchName = suggestedBranch;
		} else {
			resetState();
		}
	}}
>
	<Dialog.Content class="border-wb-hair bg-wb-panel text-wb-ink shadow-2xl sm:max-w-lg">
		<Dialog.Header class="border-b border-wb-hair pb-3">
			<Dialog.Title class="text-[14px] font-semibold text-wb-ink">Create Worktree</Dialog.Title>
			<Dialog.Description class="text-[12px] text-wb-ink-soft">
				Create a new git worktree for parallel development.
			</Dialog.Description>
		</Dialog.Header>

		<div class="flex flex-col gap-4 py-4">
			<!-- Mode toggle -->
			<div class="flex gap-1.5">
				<button
					type="button"
					onclick={() => (mode = 'new')}
					class={[
						'rounded-md border px-3 py-1 text-[12px] font-medium transition-colors',
						mode === 'new'
							? 'border-wb-accent bg-wb-accent/10 text-wb-accent'
							: 'border-wb-hair bg-wb-bg text-wb-ink-mute hover:border-wb-hair-soft hover:text-wb-ink'
					]}
				>
					New branch
				</button>
				<button
					type="button"
					onclick={() => (mode = 'existing')}
					class={[
						'rounded-md border px-3 py-1 text-[12px] font-medium transition-colors',
						mode === 'existing'
							? 'border-wb-accent bg-wb-accent/10 text-wb-accent'
							: 'border-wb-hair bg-wb-bg text-wb-ink-mute hover:border-wb-hair-soft hover:text-wb-ink'
					]}
				>
					Existing branch
				</button>
			</div>

			{#if mode === 'new'}
				<div class="flex flex-col gap-1.5">
					<Label
						for="branch-name"
						class="text-[10.5px] font-medium tracking-wide text-wb-ink-soft uppercase"
					>
						Branch name
					</Label>
					<Input
						id="branch-name"
						placeholder="feature/my-branch"
						bind:value={newBranchName}
						autocorrect="off"
						autocapitalize="off"
						spellcheck="false"
						class="border-wb-hair bg-wb-bg font-mono text-[12px] text-wb-ink placeholder:text-wb-ink-soft focus-visible:ring-wb-accent/40"
					/>
				</div>
			{:else}
				<div class="flex flex-col gap-1.5">
					<Label class="text-[10.5px] font-medium tracking-wide text-wb-ink-soft uppercase">
						Branch
					</Label>
					<Select.Root
						type="single"
						value={selectedBranch}
						onValueChange={(v) => (selectedBranch = v)}
					>
						<Select.Trigger
							class="border-wb-hair bg-wb-bg font-mono text-[12px] text-wb-ink data-[placeholder]:text-wb-ink-soft"
						>
							{selectedBranch || 'Select a branch'}
						</Select.Trigger>
						<Select.Content class="border-wb-hair bg-wb-panel">
							{#each localBranches as branch (branch.name)}
								<Select.Item
									value={branch.name}
									class="font-mono text-[12px] text-wb-ink hover:bg-wb-panel2 focus:bg-wb-panel2"
								>
									{branch.name}
								</Select.Item>
							{/each}
						</Select.Content>
					</Select.Root>
				</div>
			{/if}

			{#if worktreePath}
				<div class="flex flex-col gap-1.5">
					<Label class="text-[10.5px] font-medium tracking-wide text-wb-ink-soft uppercase">
						Worktree path
					</Label>
					<Input
						value={worktreePath}
						readonly
						class="border-wb-hair bg-wb-bg font-mono text-[11.5px] text-wb-ink-mute"
					/>
				</div>
			{/if}

			<!-- Copy options -->
			<div class="flex flex-col gap-3 rounded-md border border-wb-hair bg-wb-bg/60 p-3">
				<span class="text-[10.5px] font-medium tracking-wide text-wb-ink-soft uppercase">
					Copy untracked workspace files
				</span>
				<div class="flex items-center justify-between gap-3">
					<div>
						<p class="text-[12px] font-medium text-wb-ink">AI config</p>
						<p class="text-[11px] text-wb-ink-soft">
							Copy <code class="font-mono text-wb-ink-mute">.claude</code>,
							<code class="font-mono text-wb-ink-mute">CLAUDE.md</code>,
							<code class="font-mono text-wb-ink-mute">.codex</code>, and
							<code class="font-mono text-wb-ink-mute">.mcp.json</code> (skips git-tracked files).
						</p>
					</div>
					<Switch checked={copyAiConfig} onCheckedChange={(v) => (copyAiConfig = v)} />
				</div>
				<div class="border-t border-wb-hair-soft"></div>
				<div class="flex items-center justify-between gap-3">
					<div>
						<p class="text-[12px] font-medium text-wb-ink">Env files</p>
						<p class="text-[11px] text-wb-ink-soft">
							Copy <code class="font-mono text-wb-ink-mute">.env*</code>,
							<code class="font-mono text-wb-ink-mute">.envrc</code>, and
							<code class="font-mono text-wb-ink-mute">.dev.vars</code>.
						</p>
					</div>
					<Switch checked={copyEnvFiles} onCheckedChange={(v) => (copyEnvFiles = v)} />
				</div>
			</div>

			{#if error}
				<p class="text-[12px] text-wb-err">{error}</p>
			{/if}
		</div>

		<Dialog.Footer class="border-t border-wb-hair pt-3">
			<button
				type="button"
				onclick={() => (open = false)}
				class="rounded-md border border-wb-hair px-3 py-1.5 text-[12px] font-medium text-wb-ink transition-colors hover:bg-wb-panel2"
			>
				Cancel
			</button>
			<button
				type="button"
				onclick={handleSave}
				disabled={!branchName}
				class="rounded-md bg-primary px-3 py-1.5 text-[12px] font-medium text-primary-foreground transition-colors hover:bg-primary/90 disabled:cursor-not-allowed disabled:opacity-40"
			>
				Create Worktree
			</button>
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>
