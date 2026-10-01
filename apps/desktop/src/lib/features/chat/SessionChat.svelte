<script lang="ts">
	import type { Attachment } from 'svelte/attachments';
	import { watch } from 'runed';
	import { OverlayScrollbars } from 'overlayscrollbars';
	import { overlayScrollbars } from '$lib/utils/overlay-scrollbars';
	import BotIcon from '@lucide/svelte/icons/bot';
	import {
		activity,
		ChatComposer,
		ChatModelPicker,
		ChatPlan,
		ChatTasks,
		ChatTranscript,
		ChatUsage,
		contextUsed,
		formatTokens,
		isRunning,
		latestTodos,
		limitNotice,
		PlanUsage,
		setChatPlatform
	} from '@workbench/chat-ui';
	import { cn } from '@workbench/ui';
	import type {
		ChatImage,
		DiscoveredClaudeSession,
		ProjectConfig,
		SlashCommand
	} from '$types/workbench';
	import {
		getClaudeSessionStore,
		getWorkbenchSettingsStore,
		getWorkspaceStore
	} from '$stores/context';
	import { planUsage } from './agent-api';
	import { acquireChat } from './chat-registry';
	import { desktopChatPlatform } from './chat-platform';
	import ChatResumePicker from './ChatResumePicker.svelte';

	let {
		paneId,
		sessionId,
		project,
		cwd,
		claudeAccountId,
		onShowTerminal,
		onSessionIdChange
	}: {
		paneId: string;
		/** Fixed for this component's life — the parent re-keys on a new session. */
		sessionId: string;
		project: ProjectConfig;
		cwd?: string;
		/** The pane's Claude account; chat runs under the same login as its terminal. */
		claudeAccountId?: string;
		onShowTerminal: () => void;
		/** `/clear` moved the conversation to a new session id. */
		onSessionIdChange: (sessionId: string) => void;
	} = $props();

	setChatPlatform(desktopChatPlatform);

	const workdir = $derived(cwd ?? project.path);
	const claudeSessionStore = getClaudeSessionStore();
	const settingsStore = getWorkbenchSettingsStore();
	const workspaceStore = getWorkspaceStore();
	// svelte-ignore state_referenced_locally
	const { chat } = acquireChat(paneId, {
		projectPath: project.path,
		...(cwd && cwd !== project.path ? { worktreePath: cwd } : {}),
		sessionId,
		paneId,
		...(claudeAccountId ? { claudeAccountId } : {}),
		// The same --permission-mode terminal launches get from Settings.
		...(settingsStore.claudePermissionMode !== 'default'
			? { permissionMode: settingsStore.claudePermissionMode }
			: {}),
		// Another device's chat: join its process, never start one behind its back.
		...(workspaceStore.isAdoptedPane(paneId) ? { attachOnly: true } : {})
	});
	chat.onNeedsYou = (waiting) => claudeSessionStore.setAwaitingInput(paneId, waiting);
	chat.onTakeOver = () => workspaceStore.takeOverPane(paneId);

	watch(
		() => chat.sessionId,
		(id) => {
			if (id && id !== sessionId) onSessionIdChange(id);
		}
	);

	/** `/resume` is a terminal picker the CLI doesn't offer in chat, so the app provides it. */
	const RESUME: SlashCommand = {
		name: 'resume',
		description: 'Continue an earlier conversation from this folder'
	};
	let resumeOpen = $state(false);
	const commandList = $derived([RESUME, ...chat.commands.filter((c) => c.name !== 'resume')]);

	function clientCommand(name: string): boolean {
		if (name !== 'resume') return false;
		resumeOpen = true;
		return true;
	}

	async function earlierSessions(): Promise<DiscoveredClaudeSession[]> {
		const all = await claudeSessionStore.peekSessions(workdir, 'claude');
		return all
			.filter(
				(s) => s.sessionId !== chat.sessionId && (s.accountId ?? '') === (claudeAccountId ?? '')
			)
			.sort((a, b) => b.timestamp.localeCompare(a.timestamp));
	}

	function resume(session: DiscoveredClaudeSession) {
		resumeOpen = false;
		void workspaceStore.resumeInChat(paneId, session.sessionId, session.label);
	}

	let draft = $state('');
	let stickToBottom = true;
	/** The tasks panel as an overlay, for panes too narrow to dock it. */
	let tasksOpen = $state(false);
	/** Wide enough to dock the tasks panel; one panel is mounted, since it polls task output. */
	let wide = $state(false);
	const measureWidth: Attachment<HTMLElement> = (node) => {
		const observer = new ResizeObserver(([entry]) => (wide = entry.contentRect.width >= 1024));
		observer.observe(node);
		return () => observer.disconnect();
	};

	const limit = $derived(
		limitNotice(chat.meta?.rateLimit ?? null, (secs) =>
			new Date(secs * 1000).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })
		)
	);
	const tasks = $derived(chat.meta?.tasks ?? []);
	const runningTasks = $derived(tasks.filter(isRunning).length);
	const todos = $derived(latestTodos(chat.items));
	const now = $derived(activity(chat.items, chat.meta));
	const live = $derived(chat.status === 'live');
	const contextShare = $derived(contextUsed(chat.meta));
	const usage = new PlanUsage(
		() => planUsage(claudeAccountId),
		() => chat.meta
	);
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
				return now.kind === 'approval' ? 'Answer Claude above first' : null;
		}
	});

	/**
	 * Keep the newest message in view, unless the reader has scrolled up.
	 * Attached after `overlayScrollbars()`, so it scrolls the viewport that
	 * library generates rather than the host element.
	 */
	const followLatest: Attachment<HTMLDivElement> = (host) => {
		const node = OverlayScrollbars(host)?.elements().viewport ?? host;
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

	function send(text: string, images: ChatImage[]): boolean {
		stickToBottom = true;
		return chat.prompt(text, images);
	}

	function onKeydown(event: KeyboardEvent) {
		if (event.key === 'Escape' && chat.meta?.busy && now.kind !== 'approval') {
			event.preventDefault();
			chat.interrupt();
		}
	}
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
	{@attach measureWidth}
	class="relative flex h-full min-h-0 flex-col bg-wb-bg text-sm text-wb-ink"
	onkeydown={onKeydown}
>
	<header class="flex h-9 shrink-0 items-center gap-3 border-b border-wb-hair pr-40 pl-4 text-xs">
		<span
			class={cn(
				'size-1.5 shrink-0 rounded-full',
				live && 'bg-wb-claude',
				(chat.status === 'starting' || chat.status === 'reconnecting') &&
					'animate-pulse bg-wb-warn',
				(chat.status === 'exited' || chat.status === 'failed') && 'bg-wb-ink-soft'
			)}
		></span>
		<span class="min-w-0 truncate font-medium">{chat.meta?.title ?? 'Claude'}</span>
		{#if chat.meta?.model}
			<span class="shrink-0 text-wb-ink-soft">{chat.meta.model.replace(/\[1m\]$/, '')}</span>
		{/if}
		<div class="ml-auto flex shrink-0 items-center gap-3">
			<ChatUsage chips={usage.chips} chipClass="rounded px-0.5" />
			{#if contextShare > 0}
				<span
					class="flex shrink-0 items-center gap-1.5 text-wb-ink-soft tabular-nums"
					title="{formatTokens(chat.meta?.contextTokens ?? null)} tokens of context in use"
				>
					<span class="h-1 w-12 overflow-hidden rounded-full bg-wb-panel2">
						<span
							class={cn(
								'block h-full rounded-full',
								contextShare > 0.8 ? 'bg-wb-warn' : 'bg-wb-ink-soft'
							)}
							style:width="{contextShare * 100}%"
						></span>
					</span>
					{Math.round(contextShare * 100)}%
				</span>
			{/if}
			{#if tasks.length > 0 && !wide}
				<button
					type="button"
					class={cn(
						'flex shrink-0 items-center gap-1.5 rounded-md px-2 py-0.5 hover:bg-wb-panel2 focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none',
						runningTasks > 0 ? 'text-wb-claude' : 'text-wb-ink-mute'
					)}
					aria-expanded={tasksOpen}
					onclick={() => (tasksOpen = !tasksOpen)}
				>
					<BotIcon class="size-3.5" />
					{runningTasks > 0 ? `${runningTasks} running` : 'Tasks'}
				</button>
			{/if}
		</div>
	</header>

	<div class="flex min-h-0 flex-1">
		<div class="flex min-h-0 min-w-0 flex-1 flex-col">
			{#if chat.status === 'reconnecting'}
				<div class="reconnect relative h-0.5 shrink-0 overflow-hidden bg-wb-panel2" role="status">
					<span class="sr-only">Reconnecting to Claude</span>
				</div>
			{/if}

			<div
				{@attach overlayScrollbars()}
				{@attach followLatest}
				class="min-h-0 flex-1"
				role="log"
				aria-live="polite"
			>
				<ChatTranscript
					{chat}
					cwd={workdir}
					projectName={project.name}
					{onShowTerminal}
					onStarter={(text) => (draft = text)}
					class="mx-auto max-w-3xl px-5 py-5"
				/>
			</div>

			<div class="mx-auto flex w-full max-w-3xl shrink-0 flex-col gap-2 px-5 pb-4">
				{#if chat.notice}
					<p class="text-xs text-wb-err" role="alert">{chat.notice}</p>
				{/if}
				{#if limit}
					<p
						class={cn(
							'rounded-md border px-3 py-2 text-xs',
							limit.tone === 'blocked'
								? 'border-wb-warn/50 bg-wb-warn/10 text-wb-ink'
								: 'border-wb-hair text-wb-ink-mute'
						)}
						role={limit.tone === 'blocked' ? 'alert' : undefined}
					>
						{limit.text}
					</p>
				{/if}
				{#if todos.length > 0}
					<ChatPlan steps={todos} />
				{/if}
				<ChatComposer
					id="chat-draft-{paneId}"
					bind:draft
					mode={chat.meta?.permissionMode ?? null}
					busy={Boolean(chat.meta?.busy) && live}
					{disabledReason}
					onSend={send}
					onStop={() => chat.interrupt()}
					commands={commandList}
					onCommand={clientCommand}
					onMode={(mode) => chat.setMode(mode)}
				>
					{#snippet popover()}
						{#if resumeOpen}
							<ChatResumePicker
								load={earlierSessions}
								onPick={resume}
								onClose={() => (resumeOpen = false)}
							/>
						{/if}
					{/snippet}
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
		{#if tasks.length > 0 && wide}
			<ChatTasks {tasks} seenAt={chat.seenAt} fetchOutput={(id) => chat.taskOutput(id)} />
		{/if}
	</div>

	{#if tasksOpen && tasks.length > 0 && !wide}
		<div class="absolute inset-y-0 right-0 z-20 flex shadow-2xl">
			<ChatTasks
				{tasks}
				seenAt={chat.seenAt}
				fetchOutput={(id) => chat.taskOutput(id)}
				onClose={() => (tasksOpen = false)}
			/>
		</div>
	{/if}
</div>

<style>
	.reconnect::after {
		content: '';
		position: absolute;
		inset: 0;
		width: 30%;
		background: var(--wb-warn);
		animation: travel 1.2s ease-in-out infinite;
	}
	@keyframes travel {
		from {
			transform: translateX(-100%);
		}
		to {
			transform: translateX(340%);
		}
	}

	@media (prefers-reduced-motion: reduce) {
		.reconnect::after {
			animation: none;
		}
	}
</style>
