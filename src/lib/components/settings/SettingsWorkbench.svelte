<script lang="ts">
	import CheckIcon from '@lucide/svelte/icons/check';
	import XIcon from '@lucide/svelte/icons/x';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { Input } from '$lib/components/ui/input';
	import { Separator } from '$lib/components/ui/separator';
	import SettingsSelect from './SettingsSelect.svelte';
	import SettingsToggle from './SettingsToggle.svelte';
	import { getWorkbenchSettingsStore } from '$stores/context';
	import {
		applyClaudeIntegration,
		applyCodexIntegration,
		isNativeTerminalAvailable
	} from '$lib/utils/terminal';
	import { selectFolder } from '$lib/utils/dialog';
	import { invoke } from '@tauri-apps/api/core';
	import type {
		TerminalPerformanceMode,
		TerminalRenderer,
		WorktreeStartPoint,
		WorktreeStrategy
	} from '$types/workbench';

	import { onMount } from 'svelte';

	const store = getWorkbenchSettingsStore();

	let nativeAvailable = $state(false);
	let ghAuthenticated: boolean | null = $state(null);

	onMount(async () => {
		try {
			nativeAvailable = await isNativeTerminalAvailable();
		} catch {
			nativeAvailable = false;
		}
		ghAuthenticated = await invoke<boolean>('github_is_available');
	});

	const rendererOptions = [
		{ value: 'xterm', label: 'xterm.js (Web)' },
		{ value: 'native', label: 'Native (SwiftTerm)' }
	];

	const strategyOptions = [
		{ value: 'sibling', label: 'Sibling folder' },
		{ value: 'inside', label: 'Inside .worktrees/' }
	];
	const startPointOptions = [
		{ value: 'auto', label: 'Auto (origin default branch)' },
		{ value: 'current', label: 'Current branch' },
		{ value: 'custom', label: 'Custom branch' }
	];
	const perfModeOptions = [
		{ value: 'auto', label: 'Auto (offscreen only)' },
		{ value: 'always', label: 'Always prioritize throughput' }
	];

	async function pickCloneDir() {
		const dir = await selectFolder(
			store.cloneBaseDir ?? undefined,
			'Select Default Clone Directory'
		);
		if (dir !== null) store.set('cloneBaseDir', dir);
	}

	async function toggleClaudeHooks(checked: boolean) {
		if (checked) {
			await applyClaudeIntegration();
		}
		await store.setApproval('claude', checked);
	}

	async function toggleCodexConfig(checked: boolean) {
		if (checked) {
			await applyCodexIntegration();
		}
		await store.setApproval('codex', checked);
	}
</script>

<div class="space-y-8">
	<div class="space-y-6">
		<div>
			<h2 class="text-sm font-semibold">Features</h2>
			<p class="mt-1 text-xs text-muted-foreground">Enable or disable optional features.</p>
		</div>

		<SettingsToggle
			label="Git sidebar"
			description="Show the Git tab in the right sidebar for staging, commits, branches, and stashes."
			checked={store.gitSidebarEnabled}
			onCheckedChange={(v) => store.set('gitSidebarEnabled', v)}
		/>

		<SettingsToggle
			label="Trello"
			description="Connect your Trello boards to projects."
			checked={store.trelloEnabled}
			onCheckedChange={(v) => store.set('trelloEnabled', v)}
		/>

		<SettingsToggle
			label="Happy Coder"
			description="Use the happy CLI instead of claude, enabling remote sessions from your phone."
			checked={store.useHappyCoder}
			onCheckedChange={(v) => store.set('useHappyCoder', v)}
		/>
	</div>

	<Separator />

	<div class="space-y-6">
		<div>
			<h2 class="text-sm font-semibold">Worktrees</h2>
			<p class="mt-1 text-xs text-muted-foreground">Configure how git worktrees are created.</p>
		</div>

		<SettingsSelect
			label="Worktree location"
			description="Where new worktrees are created on disk."
			options={strategyOptions}
			value={store.worktreeStrategy}
			onValueChange={(v) => store.set('worktreeStrategy', v as WorktreeStrategy)}
		/>

		{#if store.worktreeStrategy === 'inside'}
			<p class="text-xs text-muted-foreground">
				Worktrees will be created at <code>&lt;repo&gt;/.worktrees/&lt;branch&gt;</code> and
				<code>.worktrees/</code> will be added to <code>.gitignore</code>.
			</p>
		{:else}
			<p class="text-xs text-muted-foreground">
				Worktrees will be created at <code>&lt;parent&gt;/&lt;repo&gt;-&lt;branch&gt;</code> next to the
				project folder.
			</p>
		{/if}

		<SettingsToggle
			label="Fetch before creating worktree"
			description="Run git fetch before creating a new branch to ensure it starts from the latest remote state."
			checked={store.worktreeFetchBeforeCreate}
			onCheckedChange={(checked) => store.set('worktreeFetchBeforeCreate', checked)}
		/>

		<SettingsSelect
			label="Branch new worktrees from"
			description="Start point for new worktree branches."
			options={startPointOptions}
			value={store.worktreeStartPoint}
			onValueChange={(v) => store.set('worktreeStartPoint', v as WorktreeStartPoint)}
		/>

		{#if store.worktreeStartPoint === 'custom'}
			<div class="grid gap-1.5">
				<label class="text-sm font-medium" for="custom-branch-input">Custom branch</label>
				<Input
					id="custom-branch-input"
					placeholder="e.g. develop, staging, origin/main"
					value={store.worktreeCustomBranch}
					oninput={(e) => store.set('worktreeCustomBranch', e.currentTarget.value)}
				/>
				<p class="text-xs text-muted-foreground">
					Branch or ref to use as the start point. Prefix with <code>origin/</code> to use a remote branch.
				</p>
			</div>
		{/if}
	</div>

	<Separator />

	<div class="space-y-6">
		<div>
			<h2 class="text-sm font-semibold">Terminal</h2>
			<p class="mt-1 text-xs text-muted-foreground">
				Tune xterm responsiveness. Offscreen panes always run in performance mode.
			</p>
		</div>

		{#if nativeAvailable}
			<SettingsSelect
				label="Terminal renderer"
				description="Native mode uses macOS SwiftTerm for rendering. Disables split panes. Applies to new workspaces only."
				options={rendererOptions}
				value={store.terminalRenderer}
				onValueChange={(v) => store.set('terminalRenderer', v as TerminalRenderer)}
			/>
		{/if}

		<SettingsSelect
			label="Performance mode"
			description="Auto optimizes only hidden/background panes. Always optimizes all panes."
			options={perfModeOptions}
			value={store.terminalPerformanceMode}
			onValueChange={(v) => store.set('terminalPerformanceMode', v as TerminalPerformanceMode)}
		/>

		<SettingsToggle
			label="Terminal telemetry"
			description="Log terminal throughput and latency metrics in developer console."
			checked={store.terminalTelemetryEnabled}
			onCheckedChange={(checked) => store.set('terminalTelemetryEnabled', checked)}
		/>
	</div>

	<Separator />

	<div class="space-y-4">
		<div class="flex items-center justify-between">
			<div>
				<h2 class="text-sm font-semibold">GitHub</h2>
				<p class="mt-1 text-xs text-muted-foreground">
					Clone repositories and manage PRs via the GitHub CLI.
				</p>
			</div>
			{#if ghAuthenticated === null}
				<Badge variant="outline" class="text-[10px] text-muted-foreground">Checking...</Badge>
			{:else if ghAuthenticated}
				<Badge variant="outline" class="gap-1 border-green-700 text-[10px] text-green-400">
					<CheckIcon class="size-2.5" />
					gh CLI connected
				</Badge>
			{:else}
				<Badge variant="outline" class="gap-1 border-red-700 text-[10px] text-red-400">
					<XIcon class="size-2.5" />
					gh CLI not authenticated
				</Badge>
			{/if}
		</div>

		<div class="flex items-center justify-between gap-4">
			<div class="min-w-0 flex-1">
				<p class="text-sm font-medium">Default clone directory</p>
				<p class="mt-0.5 truncate text-xs text-muted-foreground">
					{store.cloneBaseDir ?? "Not set — you'll be asked each time"}
				</p>
			</div>
			<Button variant="outline" size="sm" onclick={pickCloneDir}>Browse</Button>
		</div>

		{#if ghAuthenticated === false}
			<p class="text-[11px] text-muted-foreground">
				Run <code class="rounded bg-muted px-1 py-0.5">gh auth login</code> in a terminal to connect your
				GitHub account.
			</p>
		{/if}
	</div>

	<Separator />

	<div class="space-y-6">
		<div>
			<h2 class="text-sm font-semibold">Tool Integration</h2>
			<p class="mt-1 text-xs text-muted-foreground">
				Control whether Workbench modifies external AI tool configs.
			</p>
		</div>

		<SettingsToggle
			label="Claude hooks"
			description="Register session tracking hooks in ~/.claude/settings.json"
			checked={store.claudeHooksApproved === true}
			onCheckedChange={toggleClaudeHooks}
		/>

		<SettingsToggle
			label="Codex config"
			description="Configure CLAUDE.md fallback and notify script in ~/.codex/config/config.toml"
			checked={store.codexConfigApproved === true}
			onCheckedChange={toggleCodexConfig}
		/>
	</div>
</div>
