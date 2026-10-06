<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import type { Attachment } from 'svelte/attachments';
	import { watch } from 'runed';
	import BotIcon from '@lucide/svelte/icons/bot';
	import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
	import EllipsisVerticalIcon from '@lucide/svelte/icons/ellipsis-vertical';
	import PanelsTopLeftIcon from '@lucide/svelte/icons/panels-top-left';
	import {
		activity,
		AgentChat,
		agentName,
		awaitsAnswer,
		ChatApproval,
		ChatArtifacts,
		chatArtifacts,
		ChatComposer,
		CodexControls,
		ChatCache,
		ChatCacheHint,
		ChatContext,
		ChatElicitation,
		ChatModelPicker,
		ChatPlan,
		ChatQuestion,
		ChatSuggestions,
		ChatTasks,
		ChatTranscript,
		ChatUsage,
		contextUsed,
		isRunning,
		latestTodos,
		limitNotice,
		metaUsageChips,
		promptSuggestions,
		setChatPlatform,
		usePlanUsage
	} from '@workbench/chat-ui';
	import { cn } from '@workbench/ui';
	import * as DropdownMenu from '@workbench/ui/dropdown-menu';
	import type { ChatFile, ChatImage, TranscriptItem } from '@workbench/types';
	import { openExternal, type MobileClient } from './client.svelte.ts';
	import { baseName } from './home-format.ts';
	import type { ChatRef } from './types.ts';
	import Sheet from './Sheet.svelte';
	import ViewSwitch from './ViewSwitch.svelte';
	import { useBack } from './back-navigation';
	import ProjectReviewSheet from './ProjectReviewSheet.svelte';
	import { dictate, supportsDictation } from './dictation';

	let { client, ref }: { client: MobileClient; ref: ChatRef } = $props();

	setChatPlatform({
		openLink: openExternal,
		enterSends: false,
		...(supportsDictation() ? { dictate } : {})
	});

	// svelte-ignore state_referenced_locally
	const chat = new AgentChat(
		{
			...(ref.agent === 'codex' ? { agent: 'codex' as const } : {}),
			projectPath: ref.projectPath,
			...(ref.worktreePath ? { worktreePath: ref.worktreePath } : {}),
			...(ref.sessionId ? { sessionId: ref.sessionId } : {}),
			...(ref.claudeAccountId ? { claudeAccountId: ref.claudeAccountId } : {})
		},
		client.agents
	);
	const name = agentName(chat.agent);
	const isClaude = chat.agent === 'claude';

	const cwd = $derived(ref.worktreePath ?? ref.projectPath);
	const githubUrl = $derived(client.store?.githubUrls[ref.projectPath]);
	onMount(() => {
		void client.store?.loadGithubUrl(ref.projectPath);
	});
	const place = $derived(
		ref.worktreePath
			? `${baseName(ref.projectPath)} · ${baseName(ref.worktreePath)}`
			: baseName(ref.projectPath)
	);
	// The line under the title already names the folder, so an untitled chat falls back to its prompt.
	const title = $derived(
		chat.meta?.title ||
			chat.items.find(
				(i): i is Extract<TranscriptItem, { kind: 'user' }> => i.kind === 'user' && i.text !== ''
			)?.text ||
			name
	);
	const now = $derived(activity(chat.items, chat.meta));
	const live = $derived(chat.status === 'live');
	const tasks = $derived(chat.meta?.tasks ?? []);
	const runningTasks = $derived(tasks.filter(isRunning).length);
	const todos = $derived(latestTodos(chat.items));
	const artifacts = $derived(chatArtifacts(chat.meta?.artifacts));
	const limit = $derived(
		limitNotice(chat.meta?.rateLimit ?? null, (secs) =>
			new Date(secs * 1000).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })
		)
	);
	const contextShare = $derived(contextUsed(chat.meta));
	// Claude's plan limits come from the server's `/usage` poller; Codex reports its own in the stream.
	// svelte-ignore state_referenced_locally
	const planUsage = isClaude
		? usePlanUsage(
				`${client.connection?.url ?? ''}|${ref.claudeAccountId ?? ''}`,
				(fresh) => client.agents.usage(ref.claudeAccountId, fresh),
				() => chat.meta
			)
		: null;
	const usageChips = $derived(planUsage ? planUsage.chips : metaUsageChips(chat.meta));
	const waiting = $derived(chat.items.find(awaitsAnswer) ?? null);
	/** The sheet can be lowered to read the chat behind it; a new request raises it again. */
	let sheetHiddenFor = $state<string | null>(null);
	const sheetOpen = $derived(waiting !== null && sheetHiddenFor !== waiting.id && live);
	let tasksOpen = $state(false);
	let artifactsOpen = $state(false);
	// svelte-ignore state_referenced_locally
	const drafts = client.drafts;
	// svelte-ignore state_referenced_locally
	const draft = drafts.get(ref);
	const suggestions = $derived(promptSuggestions(chat.meta, live && !chat.rewind, draft.text));
	let reviewOpen = $state<'history' | 'changes' | null>(null);
	useBack(() => client.closeChat());
	watch(
		() => [draft.text, draft.images, draft.files],
		() => drafts.save(ref, draft)
	);
	watch(
		() => chat.sessionId,
		(id) => {
			if (!id || id === ref.sessionId) return;
			drafts.move(ref, { ...ref, sessionId: id }, draft);
			client.updateChatId(id);
		}
	);
	const commands = $derived([
		{ name: 'resume', description: 'Continue an earlier conversation' },
		...chat.commands.filter((c) => c.name !== 'resume')
	]);
	let stickToBottom = true;

	const disabledReason = $derived.by(() => {
		switch (chat.status) {
			case 'starting':
				return `Starting ${name}…`;
			case 'reconnecting':
				return 'Reconnecting…';
			case 'trust':
				return 'Trust the folder to start';
			case 'exited':
			case 'failed':
				return 'Restart the session to send messages';
			default:
				return waiting ? `Answer ${name} first` : null;
		}
	});

	/** `/clear` may have moved the conversation to a new id since this screen opened. */
	function showTerminal() {
		void client.showAsTerminal({ ...ref, sessionId: chat.sessionId }, chat.hasHistory);
	}

	function send(text: string, images: ChatImage[], files: ChatFile[]): boolean {
		stickToBottom = true;
		return chat.prompt(text, images, files);
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
	onDestroy(() => {
		drafts.save(ref, draft);
		chat.dispose();
	});
</script>

<svelte:document onvisibilitychange={onVisibility} />

<div
	class="flex h-full flex-col bg-wb-bg text-sm text-wb-ink"
	style:--wb-agent={isClaude ? undefined : 'var(--wb-codex)'}
>
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
			<span class="truncate text-[14px] font-semibold">{title}</span>
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
		{#if isClaude}
			<ViewSwitch view="chat" disabled={client.switching} onSwitch={showTerminal} />
		{/if}
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
				{#if githubUrl}
					<DropdownMenu.Item onSelect={() => openExternal(githubUrl)}
						>Open on GitHub</DropdownMenu.Item
					>
				{/if}
				<DropdownMenu.Item onSelect={() => (reviewOpen = 'history')}
					>Conversation history</DropdownMenu.Item
				>
				<DropdownMenu.Item onSelect={() => (reviewOpen = 'changes')}
					>Review changes</DropdownMenu.Item
				>
				<DropdownMenu.Item onSelect={() => chat.open()}>Restart {name}</DropdownMenu.Item>
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
			<span class="sr-only">{live ? `${name} is working` : 'Reconnecting'}</span>
		</div>
	{/if}

	<div {@attach followLatest} class="min-h-0 flex-1 overflow-y-auto" role="log" aria-live="polite">
		<ChatTranscript
			{chat}
			{cwd}
			projectName={place}
			onShowTerminal={isClaude ? showTerminal : undefined}
			onStarter={(text) => (draft.text = text)}
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
			<span class="flex-1 font-medium text-wb-warn">{name} is waiting on you</span>
			<span class="text-wb-ink-mute">Review</span>
		</button>
	{/if}

	{#if tasks.length > 0 || artifacts.length > 0 || contextShare > 0 || usageChips.length > 0}
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
			{#if artifacts.length > 0}
				<button
					type="button"
					class="flex h-7 shrink-0 items-center gap-1.5 rounded-full border border-wb-hair bg-wb-panel px-2.5 text-[11.5px] text-wb-ink-mute"
					onclick={() => (artifactsOpen = true)}
				>
					<PanelsTopLeftIcon class="size-3.5" />
					{artifacts.length === 1 ? '1 artifact' : `${artifacts.length} artifacts`}
				</button>
			{/if}
			<ChatContext meta={chat.meta} />
			<ChatCache {chat} />
			<ChatUsage chips={usageChips} chipClass="h-7 px-2.5 text-[11.5px]" />
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
		<CodexControls
			{chat}
			onThread={(sessionId, name) => client.openChat({ ...ref, sessionId, name, agent: 'codex' })}
		/>
		<ChatCacheHint {chat} />
		<ChatSuggestions {suggestions} onPick={(text) => (draft.text = text)} />
		<ChatComposer
			id="chat-draft-{ref.sessionId}"
			agent={chat.agent}
			bind:draft={draft.text}
			bind:images={draft.images}
			bind:files={draft.files}
			mode={chat.meta?.permissionMode ?? null}
			busy={Boolean(chat.meta?.busy) && live}
			{disabledReason}
			onSend={send}
			onStop={() => chat.interrupt()}
			{commands}
			onCommand={(command) => {
				if (command !== 'resume') return false;
				reviewOpen = 'history';
				return true;
			}}
			loadFiles={() => chat.listFiles()}
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
	<Sheet label="{name} is waiting on you" onClose={() => (sheetHiddenFor = waiting.id)}>
		{#if waiting.kind === 'elicitation'}
			<ChatElicitation
				elicitation={waiting}
				agent={chat.agent}
				onAnswer={(action, content) => chat.elicit(waiting.id, action, content)}
			/>
		{:else if waiting.tool === 'AskUserQuestion'}
			<ChatQuestion
				approval={waiting}
				agent={chat.agent}
				onAnswer={(decision, answers) => chat.approve(waiting.id, decision, answers)}
			/>
		{:else}
			<ChatApproval
				approval={waiting}
				agent={chat.agent}
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
			{cwd}
			fetchOutput={(id) => chat.taskOutput(id)}
			fetchTranscript={(id) => chat.taskTranscript(id)}
			onClose={() => (tasksOpen = false)}
			class="h-auto w-full rounded-lg border border-wb-hair"
		/>
	</Sheet>
{/if}

{#if artifactsOpen && artifacts.length > 0}
	<Sheet label="Artifacts" onClose={() => (artifactsOpen = false)}>
		<ChatArtifacts
			{artifacts}
			onClose={() => (artifactsOpen = false)}
			class="h-auto w-full rounded-lg border border-wb-hair"
		/>
	</Sheet>
{/if}

{#if reviewOpen}
	<ProjectReviewSheet
		{client}
		folder={{ projectPath: ref.projectPath, worktreePath: ref.worktreePath, name: place }}
		initialTab={reviewOpen}
		initialAgent={chat.agent}
		accountId={ref.claudeAccountId}
		onClose={() => (reviewOpen = null)}
	/>
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
