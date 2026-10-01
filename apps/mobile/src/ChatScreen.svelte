<script lang="ts">
	import { onDestroy } from 'svelte';
	import type { Attachment } from 'svelte/attachments';
	import { watch } from 'runed';
	import BotIcon from '@lucide/svelte/icons/bot';
	import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
	import EllipsisVerticalIcon from '@lucide/svelte/icons/ellipsis-vertical';
	import {
		activity,
		AgentChat,
		ChatApproval,
		ChatComposer,
		ChatModelPicker,
		ChatPlan,
		ChatQuestion,
		ChatTasks,
		ChatTranscript,
		ChatUsage,
		contextUsed,
		isRunning,
		latestTodos,
		limitNotice,
		setChatPlatform,
		usePlanUsage
	} from '@workbench/chat-ui';
	import { cn } from '@workbench/ui';
	import * as DropdownMenu from '@workbench/ui/dropdown-menu';
	import type { ChatImage, TranscriptItem } from '@workbench/types';
	import { baseName, openExternal, type MobileClient } from './client.svelte.ts';
	import type { ChatRef } from './types.ts';
	import Sheet from './Sheet.svelte';
	import ViewSwitch from './ViewSwitch.svelte';

	let { client, ref }: { client: MobileClient; ref: ChatRef } = $props();

	setChatPlatform({ openLink: openExternal, enterSends: false });

	// svelte-ignore state_referenced_locally
	const chat = new AgentChat(
		{
			projectPath: ref.projectPath,
			...(ref.worktreePath ? { worktreePath: ref.worktreePath } : {}),
			sessionId: ref.sessionId,
			...(ref.claudeAccountId ? { claudeAccountId: ref.claudeAccountId } : {})
		},
		client.agents
	);

	const cwd = $derived(ref.worktreePath ?? ref.projectPath);
	const place = $derived(
		ref.worktreePath
			? `${baseName(ref.projectPath)} · ${baseName(ref.worktreePath)}`
			: baseName(ref.projectPath)
	);
	const now = $derived(activity(chat.items, chat.meta));
	const live = $derived(chat.status === 'live');
	const tasks = $derived(chat.meta?.tasks ?? []);
	const runningTasks = $derived(tasks.filter(isRunning).length);
	const todos = $derived(latestTodos(chat.items));
	const limit = $derived(
		limitNotice(chat.meta?.rateLimit ?? null, (secs) =>
			new Date(secs * 1000).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })
		)
	);
	const contextShare = $derived(contextUsed(chat.meta));
	// svelte-ignore state_referenced_locally
	const usage = usePlanUsage(
		`${client.url}|${ref.claudeAccountId ?? ''}`,
		(fresh) => client.agents.usage(ref.claudeAccountId, fresh),
		() => chat.meta
	);
	const waiting = $derived(
		chat.items.find(
			(i): i is Extract<TranscriptItem, { kind: 'approval' }> =>
				i.kind === 'approval' && !i.decision && !i.expired
		) ?? null
	);
	/** The sheet can be lowered to read the chat behind it; a new request raises it again. */
	let sheetHiddenFor = $state<string | null>(null);
	const sheetOpen = $derived(waiting !== null && sheetHiddenFor !== waiting.id && live);
	let tasksOpen = $state(false);
	let draft = $state('');
	let stickToBottom = true;

	const disabledReason = $derived.by(() => {
		switch (chat.status) {
			case 'starting':
				return 'Starting Claude…';
			case 'reconnecting':
				return 'Reconnecting…';
			case 'exited':
			case 'failed':
				return 'Restart the session to send messages';
			default:
				return waiting ? 'Answer Claude first' : null;
		}
	});

	/** `/clear` may have moved the conversation to a new id since this screen opened. */
	function showTerminal() {
		void client.showAsTerminal({ ...ref, sessionId: chat.sessionId }, chat.hasHistory);
	}

	function send(text: string, images: ChatImage[]): boolean {
		stickToBottom = true;
		return chat.prompt(text, images);
	}

	const followLatest: Attachment<HTMLDivElement> = (node) => {
		const onScroll = () => {
			stickToBottom = node.scrollHeight - node.scrollTop - node.clientHeight < 48;
		};
		node.addEventListener('scroll', onScroll);
		watch(
			() => [chat.items, chat.pending, now.kind],
			() => {
				if (stickToBottom) node.scrollTop = node.scrollHeight;
			}
		);
		return () => node.removeEventListener('scroll', onScroll);
	};

	/** After the phone sleeps the socket may be dead while it still reads open. */
	let hiddenAt = 0;
	function onVisibility() {
		if (document.hidden) hiddenAt = Date.now();
		else if (Date.now() - hiddenAt > 10_000) chat.reconnect();
	}

	// The session keeps running on the server; leaving only detaches.
	onDestroy(() => chat.dispose());
</script>

<svelte:document onvisibilitychange={onVisibility} />

<div class="flex h-full flex-col bg-wb-bg text-sm text-wb-ink">
	<header
		class="flex shrink-0 items-center gap-2 border-b border-wb-hair bg-wb-rail pr-2 pl-1"
		style="padding-top: env(safe-area-inset-top); min-height: calc(3rem + env(safe-area-inset-top));"
	>
		<button
			type="button"
			class="grid size-9 shrink-0 place-items-center rounded-lg text-wb-ink-mute active:bg-wb-panel2"
			aria-label="Back"
			onclick={client.closeChat}
		>
			<ChevronLeftIcon class="size-5" />
		</button>
		<div class="flex min-w-0 flex-1 flex-col">
			<span class="truncate text-[14px] font-semibold">{chat.meta?.title ?? ref.name}</span>
			<span class="flex items-center gap-1.5 truncate font-mono text-[10.5px] text-wb-ink-soft">
				<span
					class={cn(
						'size-1.5 shrink-0 rounded-full',
						live && (waiting ? 'bg-wb-warn' : chat.meta?.busy ? 'bg-wb-accent' : 'bg-wb-ok'),
						(chat.status === 'starting' || chat.status === 'reconnecting') &&
							'animate-pulse bg-wb-warn',
						(chat.status === 'exited' || chat.status === 'failed') && 'bg-wb-ink-soft'
					)}
				></span>
				{place}
			</span>
		</div>
		<ViewSwitch view="chat" disabled={client.switching} onSwitch={showTerminal} />
		<DropdownMenu.Root>
			<DropdownMenu.Trigger>
				{#snippet child({ props })}
					<button
						{...props}
						type="button"
						class="grid size-9 shrink-0 place-items-center rounded-lg text-wb-ink-mute active:bg-wb-panel2"
						aria-label="More"
					>
						<EllipsisVerticalIcon class="size-4.5" />
					</button>
				{/snippet}
			</DropdownMenu.Trigger>
			<DropdownMenu.Content align="end" class="w-52">
				<DropdownMenu.Item onSelect={() => chat.open()}>Restart Claude</DropdownMenu.Item>
				<DropdownMenu.Item class="text-wb-err" onSelect={() => client.endChat(chat.sessionId)}>
					End session
				</DropdownMenu.Item>
			</DropdownMenu.Content>
		</DropdownMenu.Root>
	</header>

	{#if chat.status === 'reconnecting' || (live && chat.meta?.busy)}
		<div
			class={cn('bar relative h-0.5 shrink-0 overflow-hidden bg-wb-hair-soft', !live && 'warn')}
			role="status"
		>
			<span class="sr-only">{live ? 'Claude is working' : 'Reconnecting'}</span>
		</div>
	{/if}

	<div {@attach followLatest} class="min-h-0 flex-1 overflow-y-auto" role="log" aria-live="polite">
		<ChatTranscript
			{chat}
			{cwd}
			projectName={place}
			onShowTerminal={showTerminal}
			onStarter={(text) => (draft = text)}
			inlineApprovals={false}
			class="px-4 py-4"
		/>
	</div>

	{#if waiting && !sheetOpen && live}
		<button
			type="button"
			class="flex shrink-0 items-center gap-2 border-t border-wb-warn/40 bg-wb-warn/10 px-4 py-2.5 text-left text-xs"
			onclick={() => (sheetHiddenFor = null)}
		>
			<span class="size-1.5 animate-pulse rounded-full bg-wb-warn"></span>
			<span class="flex-1 font-medium text-wb-warn">Claude is waiting on you</span>
			<span class="text-wb-ink-mute">Review</span>
		</button>
	{/if}

	{#if tasks.length > 0 || contextShare > 0 || usage.chips.length > 0}
		<div class="flex shrink-0 gap-1.5 overflow-x-auto border-t border-wb-hair-soft px-3 py-1.5">
			{#if tasks.length > 0}
				<button
					type="button"
					class={cn(
						'flex h-7 shrink-0 items-center gap-1.5 rounded-full border border-wb-hair bg-wb-panel px-2.5 text-[11.5px]',
						runningTasks > 0 ? 'text-wb-claude' : 'text-wb-ink-mute'
					)}
					onclick={() => (tasksOpen = true)}
				>
					<BotIcon class="size-3.5" />
					{runningTasks > 0 ? `${runningTasks} running` : `${tasks.length} tasks`}
				</button>
			{/if}
			{#if contextShare > 0}
				<span
					class={cn(
						'flex h-7 shrink-0 items-center rounded-full border border-wb-hair px-2.5 text-[11.5px] tabular-nums',
						contextShare > 0.8 ? 'text-wb-warn' : 'text-wb-ink-soft'
					)}
				>
					Context {Math.round(contextShare * 100)}%
				</span>
			{/if}
			<ChatUsage
				chips={usage.chips}
				chipClass="h-7 rounded-full border border-wb-hair px-2.5 text-[11.5px]"
			/>
		</div>
	{/if}

	<div
		class="flex shrink-0 flex-col gap-2 border-t border-wb-hair bg-wb-rail px-2.5 pt-2"
		style="padding-bottom: calc(0.5rem + env(safe-area-inset-bottom));"
	>
		{#if chat.notice || client.notice}
			<p class="px-1 text-xs text-wb-err" role="alert">{client.notice ?? chat.notice}</p>
		{/if}
		{#if limit}
			<p
				class={cn(
					'rounded-md border px-3 py-2 text-xs',
					limit.tone === 'blocked'
						? 'border-wb-warn/50 bg-wb-warn/10 text-wb-ink'
						: 'border-wb-hair text-wb-ink-mute'
				)}
			>
				{limit.text}
			</p>
		{/if}
		{#if todos.length > 0}
			<ChatPlan steps={todos} />
		{/if}
		<ChatComposer
			id="chat-draft-{ref.sessionId}"
			bind:draft
			mode={chat.meta?.permissionMode ?? null}
			busy={Boolean(chat.meta?.busy) && live}
			{disabledReason}
			onSend={send}
			onStop={() => chat.interrupt()}
			commands={chat.commands}
			onMode={(mode) => chat.setMode(mode)}
		>
			{#snippet controls()}
				<ChatModelPicker
					meta={chat.meta}
					disabled={disabledReason !== null}
					onModel={(model) => chat.setModel(model)}
					onEffort={(effort) => chat.setEffort(effort)}
				/>
			{/snippet}
		</ChatComposer>
	</div>
</div>

{#if sheetOpen && waiting}
	<Sheet label="Claude is waiting on you" onClose={() => (sheetHiddenFor = waiting.id)}>
		{#if waiting.tool === 'AskUserQuestion'}
			<ChatQuestion
				approval={waiting}
				onAnswer={(decision, answers) => chat.approve(waiting.id, decision, answers)}
			/>
		{:else}
			<ChatApproval
				approval={waiting}
				{cwd}
				onDecide={(decision) => chat.approve(waiting.id, decision)}
			/>
		{/if}
	</Sheet>
{/if}

{#if tasksOpen && tasks.length > 0}
	<Sheet label="Agents and tasks" onClose={() => (tasksOpen = false)}>
		<ChatTasks
			{tasks}
			seenAt={chat.seenAt}
			fetchOutput={(id) => chat.taskOutput(id)}
			onClose={() => (tasksOpen = false)}
			class="h-auto w-full rounded-lg border border-wb-hair"
		/>
	</Sheet>
{/if}

<style>
	.bar::after {
		content: '';
		position: absolute;
		inset: 0;
		width: 35%;
		background: linear-gradient(90deg, transparent, var(--wb-accent), transparent);
		animation: sweep 1.4s linear infinite;
	}
	.bar.warn::after {
		background: var(--wb-warn);
	}
	@keyframes sweep {
		from {
			transform: translateX(-100%);
		}
		to {
			transform: translateX(300%);
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.bar::after {
			animation: none;
		}
	}
</style>
