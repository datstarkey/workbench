<script lang="ts">
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import * as Select from '$lib/components/ui/select';
	import { Separator } from '$lib/components/ui/separator';
	import SettingsSelect from './SettingsSelect.svelte';
	import SettingsToggle from './SettingsToggle.svelte';
	import { getWorkbenchSettingsStore } from '$stores/context';
	import { applyClaudeIntegration, applyCodexIntegration } from '$lib/utils/terminal';
	import type {
		AgentAction,
		AgentActionTarget,
		TerminalPerformanceMode,
		WorktreeStartPoint,
		WorktreeStrategy
	} from '$types/workbench';

	const store = getWorkbenchSettingsStore();

	const strategyOptions = [
		{ value: 'sibling', label: 'Sibling folder' },
		{ value: 'inside', label: 'Inside .worktrees/' }
	];
	const startPointOptions = [
		{ value: 'auto', label: 'Auto (origin default branch)' },
		{ value: 'current', label: 'Current branch' }
	];
	const perfModeOptions = [
		{ value: 'auto', label: 'Auto (offscreen only)' },
		{ value: 'always', label: 'Always prioritize throughput' }
	];

	const targetOptions = [
		{ value: 'both', label: 'Claude + Codex' },
		{ value: 'claude', label: 'Claude only' },
		{ value: 'codex', label: 'Codex only' }
	];

	function updateActionText(
		actionId: string,
		key: 'name' | 'prompt' | 'category',
		event: Event & { currentTarget: EventTarget & (HTMLInputElement | HTMLTextAreaElement) }
	) {
		if (key === 'name') {
			store.updateAgentAction(actionId, { name: event.currentTarget.value });
		} else if (key === 'category') {
			store.updateAgentAction(actionId, { category: event.currentTarget.value });
		} else {
			store.updateAgentAction(actionId, { prompt: event.currentTarget.value });
		}
	}

	function updateActionTarget(actionId: string, target: string) {
		store.updateAgentAction(actionId, { target: target as AgentActionTarget });
	}

	function targetLabel(action: AgentAction): string {
		return targetOptions.find((option) => option.value === action.target)?.label ?? action.target;
	}

	function tagsToCsv(tags: string[]): string {
		return tags.join(', ');
	}

	function updateActionTags(
		actionId: string,
		event: Event & { currentTarget: EventTarget & HTMLInputElement }
	) {
		const tags = event.currentTarget.value
			.split(',')
			.map((tag) => tag.trim())
			.filter((tag) => tag.length > 0);
		store.updateAgentAction(actionId, { tags });
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
	</div>

	<Separator />

	<div class="space-y-6">
		<div>
			<h2 class="text-sm font-semibold">Terminal Performance</h2>
			<p class="mt-1 text-xs text-muted-foreground">
				Tune xterm responsiveness. Offscreen panes always run in performance mode.
			</p>
		</div>

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

	<div>
		<h2 class="text-sm font-semibold">Integrations</h2>
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

	<Separator />

	<div class="space-y-3">
		<div class="flex items-center justify-between">
			<div>
				<h2 class="text-sm font-semibold">Agent Actions</h2>
				<p class="mt-1 text-xs text-muted-foreground">
					Reusable prompts for Claude/Codex. Launches a new session and submits the prompt
					immediately.
				</p>
				<p class="mt-1 text-xs text-muted-foreground/80">
					New installs include starter actions for PR review, DRY audits, and security scans.
				</p>
			</div>
			<Button
				type="button"
				size="sm"
				variant="outline"
				class="h-7 gap-1.5"
				onclick={() => store.addAgentAction()}
			>
				<PlusIcon class="size-3.5" />
				Add Action
			</Button>
		</div>

		{#if store.agentActions.length === 0}
			<div class="rounded-md border border-dashed p-4 text-center text-xs text-muted-foreground">
				No actions yet. Add one to quickly launch task-specific Claude or Codex sessions.
			</div>
		{:else}
			<div class="space-y-3">
				{#each store.agentActions as action (action.id)}
					<div class="rounded-md border border-border/60 bg-muted/20 p-3">
						<div class="flex items-center gap-2">
							<Input
								class="h-8"
								value={action.name}
								placeholder="Review PR for regressions"
								oninput={(event) => updateActionText(action.id, 'name', event)}
							/>
							<Select.Root
								type="single"
								value={action.target}
								onValueChange={(value) => updateActionTarget(action.id, value)}
							>
								<Select.Trigger class="h-8 w-36 text-xs">{targetLabel(action)}</Select.Trigger>
								<Select.Content>
									{#each targetOptions as option (option.value)}
										<Select.Item value={option.value}>{option.label}</Select.Item>
									{/each}
								</Select.Content>
							</Select.Root>
							<Button
								type="button"
								variant="ghost"
								size="icon-sm"
								class="size-8 text-muted-foreground hover:text-destructive"
								aria-label="Remove action"
								onclick={() => store.removeAgentAction(action.id)}
							>
								<Trash2Icon class="size-3.5" />
							</Button>
						</div>

						<div class="mt-3 grid grid-cols-1 gap-2 md:grid-cols-2">
							<div>
								<label
									class="mb-1 block text-xs font-medium text-muted-foreground"
									for={`agent-action-category-${action.id}`}
								>
									Category
								</label>
								<Input
									id={`agent-action-category-${action.id}`}
									class="h-8"
									value={action.category}
									placeholder="Code Review"
									oninput={(event) => updateActionText(action.id, 'category', event)}
								/>
							</div>
							<div>
								<label
									class="mb-1 block text-xs font-medium text-muted-foreground"
									for={`agent-action-tags-${action.id}`}
								>
									Tags (comma-separated)
								</label>
								<Input
									id={`agent-action-tags-${action.id}`}
									class="h-8"
									value={tagsToCsv(action.tags)}
									placeholder="review, regressions, tests"
									onchange={(event) => updateActionTags(action.id, event)}
								/>
							</div>
						</div>

						<div class="mt-3">
							<label
								class="mb-1 block text-xs font-medium text-muted-foreground"
								for={`agent-action-prompt-${action.id}`}
							>
								Prompt
							</label>
							<textarea
								id={`agent-action-prompt-${action.id}`}
								class="min-h-20 w-full rounded-md border border-input bg-background px-3 py-2 text-xs ring-offset-background outline-none placeholder:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring"
								value={action.prompt}
								placeholder="Review this PR looking for regressions, weak abstractions, and missing tests."
								oninput={(event) => updateActionText(action.id, 'prompt', event)}
							></textarea>
						</div>
					</div>
				{/each}
			</div>
		{/if}
	</div>
</div>
