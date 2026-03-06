<script lang="ts">
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
	import type {
		TerminalPerformanceMode,
		TerminalRenderer,
		WorktreeStartPoint,
		WorktreeStrategy
	} from '$types/workbench';

	import { onMount } from 'svelte';

	const store = getWorkbenchSettingsStore();

	let nativeAvailable = $state(false);

	onMount(async () => {
		try {
			nativeAvailable = await isNativeTerminalAvailable();
		} catch {
			nativeAvailable = false;
		}
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
			onCheckedChange={(v) => store.setGitSidebarEnabled(v)}
		/>

		<SettingsToggle
			label="Trello"
			description="Connect your Trello boards to projects."
			checked={store.trelloEnabled}
			onCheckedChange={(v) => store.setTrelloEnabled(v)}
		/>

		<SettingsToggle
			label="Happy Coder"
			description="Use the happy CLI instead of claude, enabling remote sessions from your phone."
			checked={store.useHappyCoder}
			onCheckedChange={(v) => store.setUseHappyCoder(v)}
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
			onValueChange={(v) => store.setWorktreeStrategy(v as WorktreeStrategy)}
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
			onCheckedChange={(checked) => store.setWorktreeFetchBeforeCreate(checked)}
		/>

		<SettingsSelect
			label="Branch new worktrees from"
			description="Start point for new worktree branches."
			options={startPointOptions}
			value={store.worktreeStartPoint}
			onValueChange={(v) => store.setWorktreeStartPoint(v as WorktreeStartPoint)}
		/>

		{#if store.worktreeStartPoint === 'custom'}
			<div class="grid gap-1.5">
				<label class="text-sm font-medium" for="custom-branch-input">Custom branch</label>
				<Input
					id="custom-branch-input"
					placeholder="e.g. develop, staging, origin/main"
					value={store.worktreeCustomBranch}
					oninput={(e) => store.setWorktreeCustomBranch(e.currentTarget.value)}
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
				description="Native mode uses macOS SwiftTerm for rendering. Disables split panes."
				options={rendererOptions}
				value={store.terminalRenderer}
				onValueChange={(v) => store.setTerminalRenderer(v as TerminalRenderer)}
			/>
		{/if}

		<SettingsSelect
			label="Performance mode"
			description="Auto optimizes only hidden/background panes. Always optimizes all panes."
			options={perfModeOptions}
			value={store.terminalPerformanceMode}
			onValueChange={(v) => store.setTerminalPerformanceMode(v as TerminalPerformanceMode)}
		/>

		<SettingsToggle
			label="Terminal telemetry"
			description="Log terminal throughput and latency metrics in developer console."
			checked={store.terminalTelemetryEnabled}
			onCheckedChange={(checked) => store.setTerminalTelemetryEnabled(checked)}
		/>
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
