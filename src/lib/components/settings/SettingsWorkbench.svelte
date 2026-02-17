<script lang="ts">
	import CheckIcon from '@lucide/svelte/icons/check';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import XIcon from '@lucide/svelte/icons/x';
	import { Button } from '$lib/components/ui/button';
	import { Badge } from '$lib/components/ui/badge';
	import { Input } from '$lib/components/ui/input';
	import * as Select from '$lib/components/ui/select';
	import { Separator } from '$lib/components/ui/separator';
	import SettingsSelect from './SettingsSelect.svelte';
	import SettingsToggle from './SettingsToggle.svelte';
	import { getWorkbenchSettingsStore } from '$stores/context';
	import { applyClaudeIntegration, applyCodexIntegration } from '$lib/utils/terminal';
	import { selectFolder } from '$lib/utils/dialog';
	import { invoke } from '@tauri-apps/api/core';
	import { onMount } from 'svelte';
	import type { AgentAction, AgentActionTarget, WorktreeStrategy } from '$types/workbench';

	const store = getWorkbenchSettingsStore();

	let ghAuthenticated: boolean | null = $state(null);

	onMount(async () => {
		ghAuthenticated = await invoke<boolean>('github_is_available');
	});

	const strategyOptions = [
		{ value: 'sibling', label: 'Sibling folder' },
		{ value: 'inside', label: 'Inside .worktrees/' }
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

	async function pickCloneDir() {
		const dir = await selectFolder(store.cloneBaseDir ?? undefined, 'Select Default Clone Directory');
		if (dir !== null) store.setCloneBaseDir(dir);
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
				Run <code class="rounded bg-muted px-1 py-0.5">gh auth login</code> in a terminal to connect your GitHub account.
			</p>
		{/if}
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
