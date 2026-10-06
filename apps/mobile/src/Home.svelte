<script lang="ts">
	import { onMount } from 'svelte';
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import MessageSquareIcon from '@lucide/svelte/icons/message-square';
	import SettingsIcon from '@lucide/svelte/icons/settings';
	import SquareTerminalIcon from '@lucide/svelte/icons/square-terminal';
	import XIcon from '@lucide/svelte/icons/x';
	import { Elapsed, shortPath } from '@workbench/chat-ui';
	import { cn } from '@workbench/ui';
	import type { MobileClient } from './client.svelte.ts';
	import { age, answerableFromHome, chatWhere, waitingLabel } from './home-format.ts';
	import MachinesSheet from './MachinesSheet.svelte';
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

	const needsYou = $derived(client.chats.filter((c) => !c.exited && c.waiting));
	const runningChats = $derived(client.chats.filter((c) => !c.exited && !c.waiting));
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
				{#each needsYou as chat (chat.sessionId)}
					{@const waiting = chat.waiting!}
					<div
						class="flex flex-col gap-2.5 rounded-xl border border-wb-warn/35 bg-wb-warn/[0.07] p-3"
					>
						<div class="flex items-center gap-2 text-xs">
							<span class="font-semibold text-wb-warn">{waitingLabel(waiting)}</span>
							{#if chat.agent === 'codex'}
								<span class="rounded bg-wb-panel2 px-1.5 py-px font-mono text-[10px] text-wb-codex"
									>codex</span
								>
							{/if}
							<span class="ml-auto font-mono text-[11px] text-wb-ink-soft"
								>{age(chat.updatedAt, now)}</span
							>
						</div>
						<span class="truncate text-[14px] font-semibold">{chat.title ?? chatWhere(chat)}</span>
						{#if answerableFromHome(waiting)}
							<code
								class="truncate rounded-md border border-wb-hair bg-wb-rail px-2.5 py-1.5 font-mono text-[11.5px]"
								>{shortPath(waiting.preview, chat.worktreePath ?? chat.projectPath)}</code
							>
						{/if}
						<div class="flex gap-1.5">
							{#if answerableFromHome(waiting)}
								<button
									type="button"
									class="h-9 flex-1 rounded-lg bg-wb-accent text-[13px] font-semibold text-wb-accent-ink active:brightness-90"
									onclick={() => client.answer(chat.sessionId, waiting.id, 'allow')}
								>
									Allow
								</button>
							{/if}
							<button
								type="button"
								class="h-9 flex-1 rounded-lg border border-wb-hair bg-wb-panel text-[13px] font-semibold text-wb-ink-mute active:bg-wb-panel2"
								onclick={() => client.openChat(client.chatRef(chat))}
							>
								{answerableFromHome(waiting) ? 'Open' : 'Review'}
							</button>
							{#if answerableFromHome(waiting)}
								<button
									type="button"
									class="h-9 rounded-lg border border-wb-hair bg-wb-panel px-3.5 text-[13px] font-semibold text-wb-err active:bg-wb-panel2"
									onclick={() => client.answer(chat.sessionId, waiting.id, 'deny')}
								>
									Deny
								</button>
							{/if}
						</div>
					</div>
				{/each}
			</section>
		{/if}

		{#if runningChats.length > 0 || client.standaloneTerminals.length > 0}
			<section class="flex flex-col gap-2">
				{@render sectionTitle('Running', runningChats.length + client.standaloneTerminals.length)}
				{#each runningChats as chat (chat.sessionId)}
					<button
						type="button"
						class="grid w-full grid-cols-[auto_1fr_auto] items-center gap-x-2.5 gap-y-0.5 rounded-xl border border-wb-hair-soft bg-wb-panel px-3 py-2.5 text-left active:bg-wb-panel2"
						onclick={() => client.openChat(client.chatRef(chat))}
					>
						<span
							class={cn(
								'row-span-2 grid size-8 place-items-center rounded-lg bg-wb-panel2',
								chat.agent === 'codex' ? 'text-wb-codex' : 'text-wb-claude'
							)}
						>
							<MessageSquareIcon class="size-4" />
						</span>
						<span class="truncate text-[13.5px] font-medium">{chat.title ?? 'New chat'}</span>
						<span
							class="row-span-2 flex flex-col items-end gap-1 font-mono text-[11px] text-wb-ink-soft"
						>
							{#if chat.busy && chat.busySince}
								<span class="spinner size-3"></span>
								<Elapsed since={chat.busySince} />
							{:else}
								<span class="size-1.5 rounded-full bg-wb-ink-soft"></span>
								{age(chat.updatedAt, now)}
							{/if}
						</span>
						<span class="flex min-w-0 items-center gap-1.5 text-[11.5px] text-wb-ink-mute">
							{#if chat.agent === 'codex'}
								<span
									class="shrink-0 rounded bg-wb-panel2 px-1.5 py-px font-mono text-[10px] text-wb-codex"
									>codex</span
								>
							{/if}
							<span
								class="shrink-0 rounded bg-wb-panel2 px-1.5 py-px font-mono text-[10px] text-wb-ink-mute"
								>{chatWhere(chat)}</span
							>
							{#if chat.running}
								<span class="truncate font-mono text-[11px] text-wb-ink"
									>{shortPath(chat.running.detail, chat.worktreePath ?? chat.projectPath) ||
										chat.running.name}</span
								>
							{:else if chat.busy}
								<span class="truncate">Thinking</span>
							{:else}
								<span class="truncate">Your turn</span>
							{/if}
						</span>
					</button>
				{/each}
				{#each client.standaloneTerminals as t (t.id)}
					{@const claude = client.terminalChats[t.id]}
					<div
						class="flex items-center gap-2.5 rounded-xl border border-wb-hair-soft bg-wb-panel py-2.5 pr-1.5 pl-3"
					>
						<button
							type="button"
							class="flex min-w-0 flex-1 items-center gap-2.5 text-left"
							onclick={() => client.selectTerminal(t.id)}
						>
							<span
								class={cn(
									'grid size-8 shrink-0 place-items-center rounded-lg bg-wb-panel2',
									claude ? 'text-wb-claude' : 'text-wb-shell'
								)}
							>
								<SquareTerminalIcon class="size-4" />
							</span>
							<span class="flex min-w-0 flex-col">
								<span class="truncate text-[13.5px] font-medium">{t.name ?? t.id.slice(0, 8)}</span>
								<span class="flex items-center gap-1.5 text-[11.5px] text-wb-ink-mute">
									<span
										class="rounded bg-wb-panel2 px-1.5 py-px font-mono text-[10px] text-wb-shell"
										>{claude ? 'claude · terminal' : 'terminal'}</span
									>
									{#if !t.alive}exited{/if}
								</span>
							</span>
						</button>
						<button
							type="button"
							class="grid size-8 shrink-0 place-items-center rounded-lg text-wb-ink-soft active:bg-wb-panel2 active:text-wb-err"
							aria-label="Close {t.name ?? 'terminal'}"
							onclick={() => client.killTerminal(t.id)}
						>
							<XIcon class="size-4" />
						</button>
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
					<option value="">Default</option>
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
