<script lang="ts">
	import { Badge } from '$lib/components/ui/badge';
	import { Button } from '$lib/components/ui/button';
	import { Input } from '$lib/components/ui/input';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import XIcon from '@lucide/svelte/icons/x';
	import { getClaudeSettingsStore } from '$stores/context';
	import SettingsEmptyState from './SettingsEmptyState.svelte';
	import type { HookEntry } from '$types/claude-settings';

	const claudeSettingsStore = getClaudeSettingsStore();

	let settings = $derived(claudeSettingsStore.currentSettings);
	let hooks = $derived((settings.hooks ?? {}) as Record<string, HookEntry[]>);
	let hookEvents = $derived(Object.keys(hooks));

	const knownEvents = ['PreToolUse', 'PostToolUse', 'Notification', 'Stop', 'SubagentStop'];

	let addingEvent = $state('');
	let newCommand = $state('');

	function addHookEntry(event: string) {
		const cmd = newCommand.trim();
		if (!cmd) return;
		const updated = { ...hooks };
		const entries = [...(updated[event] ?? [])];
		entries.push({ command: cmd });
		updated[event] = entries;
		claudeSettingsStore.updateNested('hooks', updated);
		newCommand = '';
		addingEvent = '';
	}

	function removeHookEntry(event: string, index: number) {
		const updated = { ...hooks };
		const entries = [...(updated[event] ?? [])];
		entries.splice(index, 1);
		if (entries.length === 0) {
			delete updated[event];
		} else {
			updated[event] = entries;
		}
		claudeSettingsStore.updateNested('hooks', updated);
	}

	function startAdding(event: string) {
		addingEvent = event;
		newCommand = '';
	}

	let availableEvents = $derived(knownEvents.filter((e) => !hookEvents.includes(e)));
</script>

<div class="space-y-6">
	<p class="text-xs text-muted-foreground">
		Hooks run shell commands in response to Claude Code events.
	</p>

	{#if hookEvents.length === 0 && availableEvents.length > 0}
		<SettingsEmptyState title="No hooks configured." />
	{/if}

	{#each hookEvents as event (event)}
		<div>
			<div class="flex items-center gap-2">
				<h3 class="text-sm font-medium">{event}</h3>
				<Badge variant="secondary" class="text-[10px]">{hooks[event].length}</Badge>
			</div>
			<div class="mt-2 space-y-1.5">
				{#each hooks[event] as entry, i (i)}
					<div class="flex items-center gap-1.5">
						<div
							class="flex flex-1 items-center gap-2 rounded-md border border-border/60 px-2 py-1"
						>
							<code class="flex-1 text-xs">{entry.command}</code>
							{#if entry.matcher}
								<Badge variant="outline" class="text-[10px]">{entry.matcher}</Badge>
							{/if}
						</div>
						<Button
							variant="ghost"
							size="icon-sm"
							class="size-6 shrink-0 text-muted-foreground hover:text-destructive"
							onclick={() => removeHookEntry(event, i)}
						>
							<XIcon class="size-3" />
						</Button>
					</div>
				{/each}
				{#if addingEvent === event}
					<form
						class="flex items-center gap-1.5"
						onsubmit={(e) => {
							e.preventDefault();
							addHookEntry(event);
						}}
					>
						<Input
							class="h-7 flex-1 font-mono text-xs"
							placeholder="Command to run"
							bind:value={newCommand}
						/>
						<Button variant="outline" size="icon-sm" class="size-7 shrink-0" type="submit">
							<PlusIcon class="size-3" />
						</Button>
					</form>
				{:else}
					<Button
						variant="ghost"
						size="sm"
						class="h-7 gap-1.5 text-xs text-muted-foreground"
						onclick={() => startAdding(event)}
					>
						<PlusIcon class="size-3" />
						Add command
					</Button>
				{/if}
			</div>
		</div>
	{/each}

	{#if availableEvents.length > 0}
		<div>
			<h3 class="text-xs font-medium text-muted-foreground">Add Hook Event</h3>
			<div class="mt-2 flex flex-wrap gap-1.5">
				{#each availableEvents as event (event)}
					<Button
						variant="outline"
						size="sm"
						class="h-7 gap-1.5 text-xs"
						onclick={() => {
							const updated = { ...hooks, [event]: [] };
							claudeSettingsStore.updateNested('hooks', updated);
							startAdding(event);
						}}
					>
						<PlusIcon class="size-3" />
						{event}
					</Button>
				{/each}
			</div>
		</div>
	{/if}

	{#if claudeSettingsStore.hookScripts.length > 0}
		<div>
			<h3 class="text-xs font-medium text-muted-foreground">Hook Scripts on Disk</h3>
			<div class="mt-2 space-y-1">
				{#each claudeSettingsStore.hookScripts as script (script.path)}
					<div class="rounded-md border border-border/60 px-2 py-1">
						<code class="text-xs">{script.name}</code>
					</div>
				{/each}
			</div>
		</div>
	{/if}
</div>
