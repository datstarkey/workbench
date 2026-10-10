<script lang="ts">
	import { AgentIcon } from '@workbench/ui/agent-icon';
	import { onMount } from 'svelte';
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import SettingsIcon from '@lucide/svelte/icons/settings';
	import SquareTerminalIcon from '@lucide/svelte/icons/square-terminal';
	import XIcon from '@lucide/svelte/icons/x';
	import { Elapsed, shortPath } from '@workbench/chat-ui';
	import { cn } from '@workbench/ui';
	import type { MobileClient } from './client.svelte.ts';
	import {
		age,
		answerableFromHome,
		runningTasksLabel,
		totalRunningTasks,
		waitingLabel
	} from './home-format.ts';
	import MachinesSheet from './MachinesSheet.svelte';
	import { paneTitle, workspaceLabel, type PaneEntry } from './panes.ts';
	import ProjectList from './ProjectList.svelte';
	import Sheet from './Sheet.svelte';
	import ProjectReviewSheet from './ProjectReviewSheet.svelte';
	import type { ReviewFolder } from './project-review.svelte';

	let { client }: { client: MobileClient } = $props();

	const store = $derived(client.store!);
	let settingsOpen = $state(false);
	let machinesOpen = $state(false);
	let reviewFolder = $state<ReviewFolder | null>(null);
	let reviewTab = $state<'history' | 'changes'>('history');
	const machineName = $derived(client.machine?.name ?? client.connection?.url ?? '');
	const status = $derived(
		client.connecting ? 'switching' : client.online ? 'connected' : 'not responding'
	);

	/** Re-render relative times without refetching. */
	let now = $state(Date.now());
	onMount(() => {
		const timer = setInterval(() => (now = Date.now()), 30_000);
		return () => clearInterval(timer);
	});

	const needsYou = $derived(client.panes.filter((e) => e.pane.waiting && e.pane.sessionId));
	/** The host's workspaces as it orders them, each with its panes. */
	const running = $derived(
		(client.remote?.workspaces ?? [])
			.map((workspace) => ({
				workspace,
				entries: client.panes.filter((e) => e.workspace === workspace && !e.pane.waiting)
			}))
			.filter((group) => group.entries.length > 0)
	);
	const runningCount = $derived(running.reduce((n, group) => n + group.entries.length, 0));
	/** Every session's unfinished subagents and tasks on this machine, for the top bar. */
	const machineTasks = $derived(
		runningTasksLabel(totalRunningTasks(client.panes.map((e) => e.pane.runningTasks)))
	);

	function activity({ pane, workspace }: PaneEntry): string {
		if (pane.status === 'needsTrust') return 'Needs folder trust';
		if (pane.status === 'starting') return 'Starting';
		if (pane.status === 'exited') return 'Exited';
		if (pane.kind === 'shell') return 'Terminal';
		if (pane.running)
			return (
				shortPath(pane.running.detail, workspace.worktreePath ?? workspace.projectPath) ||
				pane.running.name
			);
		return pane.busy ? 'Thinking' : 'Your turn';
	}
</script>

{#snippet sectionTitle(label: string, count?: number, dot?: boolean)}
	<h2
		class="flex items-center gap-2 px-0.5 text-[11px] font-semibold tracking-[0.08em] text-wb-ink-soft uppercase"
	>
		{#if dot}<span class="size-1.5 animate-pulse rounded-full bg-wb-warn"></span>{/if}
		{label}
		{#if count !== undefined}<span class="font-mono tracking-normal text-wb-ink-mute">{count}</span
			>{/if}
	</h2>
{/snippet}

<div class="flex h-full flex-col bg-wb-bg text-wb-ink">
	<header
		class="flex shrink-0 items-center gap-2 border-b border-wb-hair bg-wb-rail pr-2 pl-4"
		style="padding-top: env(safe-area-inset-top); min-height: calc(3rem + env(safe-area-inset-top));"
	>
		<span class="text-[15px] font-semibold tracking-tight">Workbench</span>
		{#if machineTasks}
			<span
				class="flex min-w-0 shrink items-center gap-1.5 rounded-full bg-wb-panel2 px-2 py-0.5 text-[11px] text-wb-ink-mute"
				aria-label="Running on this machine: {machineTasks}"
			>
				<span class="size-1.5 shrink-0 animate-pulse rounded-full bg-wb-accent"></span>
				<span class="truncate">{machineTasks}</span>
			</span>
		{/if}
		<button
			type="button"
			class="ml-auto flex h-9 min-w-0 items-center gap-1.5 rounded-full border border-wb-hair px-3 text-[12px] text-wb-ink-mute active:bg-wb-panel2"
			aria-label="Machine: {machineName}, {status}. Switch machine"
			onclick={() => (machinesOpen = true)}
		>
			<span
				class={cn(
					'size-1.5 shrink-0 rounded-full',
					client.connecting ? 'animate-pulse bg-wb-warn' : client.online ? 'bg-wb-ok' : 'bg-wb-err'
				)}
			></span>
			<span class="truncate font-medium">{machineName}</span>
			<ChevronDownIcon class="size-3.5 shrink-0" />
		</button>
		<button
			type="button"
			class="grid size-9 shrink-0 place-items-center rounded-lg text-wb-ink-mute active:bg-wb-panel2"
			aria-label="Settings"
			onclick={() => (settingsOpen = true)}
		>
			<SettingsIcon class="size-4.5" />
		</button>
	</header>

	<main class="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto px-3.5 py-4">
		{#if store.error || client.notice}
			<div
				class="flex items-start gap-2 rounded-lg border border-wb-err/40 bg-wb-err/10 py-1 pr-1 pl-3 text-xs text-wb-err"
				role="alert"
			>
				<span class="min-w-0 flex-1 py-1.5 break-words">{client.notice ?? store.error}</span>
				{#if client.notice}
					<button
						type="button"
						class="grid size-7 shrink-0 place-items-center rounded-md active:bg-wb-err/20"
						aria-label="Dismiss"
						onclick={() => (client.notice = null)}
					>
						<XIcon class="size-3.5" />
					</button>
				{/if}
			</div>
		{/if}

		{#if needsYou.length > 0}
			<section class="flex flex-col gap-2">
				{@render sectionTitle('Needs you', needsYou.length, true)}
				{#each needsYou as entry (entry.pane.id)}
					{@const pane = entry.pane}
					{@const waiting = pane.waiting!}
					{@const sessionId = pane.sessionId!}
					<div
						class="flex flex-col gap-2.5 rounded-xl border border-wb-warn/35 bg-wb-warn/[0.07] p-3"
					>
						<div class="flex items-center gap-2 text-xs">
							{#if pane.kind !== 'shell'}<AgentIcon agent={pane.kind} class="size-3.5" />{/if}
							<span class="font-semibold text-wb-warn">{waitingLabel(waiting)}</span>
							{#if pane.kind === 'codex'}
								<span class="rounded bg-wb-panel2 px-1.5 py-px font-mono text-[10px] text-wb-codex"
									>codex</span
								>
							{/if}
							{#if pane.waitingSince}
								<span class="ml-auto font-mono text-[11px] text-wb-ink-soft"
									>{age(pane.waitingSince, now)}</span
								>
							{/if}
						</div>
						<span class="truncate text-[14px] font-semibold">{paneTitle(entry)}</span>
						{#if answerableFromHome(waiting)}
							<code
								class="truncate rounded-md border border-wb-hair bg-wb-rail px-2.5 py-1.5 font-mono text-[11.5px]"
								>{shortPath(
									waiting.preview,
									entry.workspace.worktreePath ?? entry.workspace.projectPath
								)}</code
							>
						{/if}
						<div class="flex gap-1.5">
							{#if answerableFromHome(waiting)}
								<button
									type="button"
									class="h-9 flex-1 rounded-lg bg-wb-accent text-[13px] font-semibold text-wb-accent-ink active:brightness-90"
									onclick={() => client.answer(sessionId, waiting.id, 'allow')}
								>
									Allow
								</button>
							{/if}
							<button
								type="button"
								class="h-9 flex-1 rounded-lg border border-wb-hair bg-wb-panel text-[13px] font-semibold text-wb-ink-mute active:bg-wb-panel2"
								onclick={() => client.openPane(pane.id, waiting.inTerminal ? 'terminal' : 'chat')}
							>
								{answerableFromHome(waiting) ? 'Open' : 'Review'}
							</button>
							{#if answerableFromHome(waiting)}
								<button
									type="button"
									class="h-9 rounded-lg border border-wb-hair bg-wb-panel px-3.5 text-[13px] font-semibold text-wb-err active:bg-wb-panel2"
									onclick={() => client.answer(sessionId, waiting.id, 'deny')}
								>
									Deny
								</button>
							{/if}
						</div>
					</div>
				{/each}
			</section>
		{/if}

		{#if running.length > 0}
			<section class="flex flex-col gap-2">
				{@render sectionTitle('Running', runningCount)}
				{#each running as group (group.workspace.id)}
					<div class="flex flex-col gap-2">
						<h3 class="truncate px-0.5 pt-1 font-mono text-[11px] text-wb-ink-mute">
							{workspaceLabel(group.workspace)}
						</h3>
						{#each group.entries as entry (entry.pane.id)}
							{@const pane = entry.pane}
							{@const title = paneTitle(entry)}
							{@const tasksLabel = runningTasksLabel(pane.runningTasks)}
							<div
								class="flex items-center gap-2.5 rounded-xl border border-wb-hair-soft bg-wb-panel py-2.5 pr-1.5 pl-3"
							>
								<button
									type="button"
									class="flex min-w-0 flex-1 items-center gap-2.5 text-left"
									onclick={() => client.openPane(pane.id)}
								>
									<span
										class={cn(
											'grid size-8 shrink-0 place-items-center rounded-lg bg-wb-panel2',
											pane.kind === 'codex'
												? 'text-wb-codex'
												: pane.kind === 'claude'
													? 'text-wb-claude'
													: 'text-wb-shell'
										)}
									>
										{#if pane.kind === 'shell'}
											<SquareTerminalIcon class="size-4" />
										{:else}
											<AgentIcon agent={pane.kind} class="size-4" />
										{/if}
									</span>
									<span class="flex min-w-0 flex-col">
										<span class="truncate text-[13.5px] font-medium">{title}</span>
										<span
											class={cn(
												'truncate text-[11.5px] text-wb-ink-mute',
												pane.running && 'font-mono text-[11px] text-wb-ink'
											)}>{activity(entry)}</span
										>
										{#if tasksLabel}
											<span class="flex items-center gap-1.5 text-[11px] text-wb-ink-soft">
												<span class="size-1.5 shrink-0 animate-pulse rounded-full bg-wb-accent"
												></span>
												<span class="truncate">{tasksLabel}</span>
											</span>
										{/if}
									</span>
									<span
										class="ml-auto flex shrink-0 flex-col items-end gap-1 font-mono text-[11px] text-wb-ink-soft"
									>
										{#if pane.busy && pane.busySince}
											<span class="spinner size-3"></span>
											<Elapsed since={pane.busySince} />
										{:else if pane.turnEndedAt}
											{age(pane.turnEndedAt, now)}
										{/if}
									</span>
								</button>
								<!-- Ending a Claude or Codex session is in its chat's menu, not one stray tap away. -->
								{#if pane.kind === 'shell'}
									<button
										type="button"
										class="grid size-8 shrink-0 place-items-center rounded-lg text-wb-ink-soft active:bg-wb-panel2 active:text-wb-err"
										aria-label="Close {title}"
										onclick={() => client.endPane(pane.id)}
									>
										<XIcon class="size-4" />
									</button>
								{/if}
							</div>
						{/each}
					</div>
				{/each}
			</section>
		{/if}

		<ProjectList
			{client}
			onReview={(folder, tab) => {
				reviewTab = tab;
				reviewFolder = folder;
			}}
		/>
	</main>
</div>

{#if machinesOpen}
	<MachinesSheet {client} onClose={() => (machinesOpen = false)} />
{/if}

{#if settingsOpen}
	<Sheet label="Settings" onClose={() => (settingsOpen = false)}>
		<div class="flex flex-col gap-5 pb-2">
			<div class="flex flex-col gap-2">
				<span class="text-[13px] font-semibold">Open new Claude sessions as</span>
				<div class="grid grid-cols-2 gap-1 rounded-xl border border-wb-hair bg-wb-panel2 p-1">
					{#each [{ id: 'chat', label: 'Chat' }, { id: 'terminal', label: 'Terminal' }] as const as option (option.id)}
						<button
							type="button"
							class={cn(
								'h-9 rounded-lg text-[13px] font-medium',
								client.defaultView === option.id
									? 'bg-wb-bg text-wb-ink shadow-sm'
									: 'text-wb-ink-soft'
							)}
							aria-pressed={client.defaultView === option.id}
							onclick={() => client.setDefaultView(option.id)}
						>
							{option.label}
						</button>
					{/each}
				</div>
				<p class="text-xs text-wb-ink-mute">
					Claude sessions can switch between chat and terminal from their header.
				</p>
			</div>
			{#if client.notifications.supported}
				<label class="flex items-center gap-3 text-[13px] font-semibold">
					<input
						type="checkbox"
						checked={client.notifications.enabled}
						onchange={(e) => client.notifications.setEnabled(e.currentTarget.checked)}
						class="size-4 accent-wb-accent"
					/>
					Approval and completion notifications
				</label>
				<p class="text-xs text-wb-ink-mute">
					Watch every session on the connected machine while the app is in the background. Android
					shows an ongoing notification while monitoring is active.
				</p>
				{#if client.notifications.error}<p role="alert" class="text-xs text-wb-err">
						{client.notifications.error}
					</p>{/if}
			{/if}
			<label class="flex flex-col gap-2 text-[13px] font-semibold"
				>Claude account for new sessions
				<select
					class="h-10 rounded-lg border border-wb-hair bg-wb-panel2 px-3 font-normal"
					value={client.accountId ?? ''}
					onchange={(e) => client.setAccount(e.currentTarget.value)}
				>
					<option value="">{client.defaultAccountName}</option>
					{#each client.accounts as account (account.id)}<option value={account.id}
							>{account.name}</option
						>{/each}
				</select>
			</label>
			<div class="flex flex-col gap-1">
				<span class="text-[13px] font-semibold">Server</span>
				<span class="font-mono text-xs break-all text-wb-ink-mute">{client.connection?.url}</span>
			</div>
			<div class="flex gap-2">
				<button
					type="button"
					class="h-10 flex-1 rounded-lg border border-wb-hair bg-wb-panel2 text-[13px] font-medium active:bg-wb-panel"
					onclick={() => {
						client.refreshAll();
						settingsOpen = false;
					}}
				>
					Refresh
				</button>
				<button
					type="button"
					class="h-10 flex-1 rounded-lg border border-wb-hair bg-wb-panel2 text-[13px] font-medium text-wb-err active:bg-wb-panel"
					onclick={() => {
						settingsOpen = false;
						client.disconnect();
					}}
				>
					Disconnect
				</button>
			</div>
		</div>
	</Sheet>
{/if}

{#if reviewFolder}
	<ProjectReviewSheet
		{client}
		folder={reviewFolder}
		initialTab={reviewTab}
		accountId={client.accountId}
		onClose={() => (reviewFolder = null)}
	/>
{/if}

<style>
	.spinner {
		border-radius: 50%;
		border: 1.5px solid var(--wb-accent-soft);
		border-top-color: var(--wb-accent);
		animation: spin 0.8s linear infinite;
	}
	@keyframes spin {
		to {
			transform: rotate(1turn);
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.spinner {
			animation: none;
		}
	}
</style>
