<script lang="ts">
	import type { Attachment } from 'svelte/attachments';
	import { watch } from 'runed';
	import { OverlayScrollbars } from 'overlayscrollbars';
	import { overlayScrollbars } from '$lib/utils/overlay-scrollbars';
	import BotIcon from '@lucide/svelte/icons/bot';
	import PanelsTopLeftIcon from '@lucide/svelte/icons/panels-top-left';
	import GitBranchIcon from '@lucide/svelte/icons/git-branch';
	import {
		activity,
		agentName,
		ChatArtifacts,
		chatArtifacts,
		ChatComposer,
		ChatCache,
		ChatCacheHint,
		ChatContext,
		ChatModelPicker,
		ChatPlan,
		ChatSuggestions,
		ChatTasks,
		ChatTranscript,
		ChatUsage,
		isRunning,
		latestTodos,
		limitNotice,
		metaUsageChips,
		promptSuggestions,
		setChatPlatform,
		usePlanUsage
	} from '@workbench/chat-ui';
	import { cn } from '@workbench/ui';
	import type {
		AgentKind,
		ChatFile,
		ChatImage,
		DiscoveredClaudeSession,
		ProjectConfig,
		SlashCommand
	} from '$types/workbench';
	import {
		getClaudeSessionStore,
		getGitHubStore,
		getWorkbenchSettingsStore,
		getWorkspaceStore
	} from '$stores/context';
	import PRStatusBadge from '$features/projects/PRStatusBadge.svelte';
	import { codexChatMode } from '$lib/utils/claude';
	import { openUrl } from '$lib/utils/open-url';
	import { planUsage } from './agent-api';
	import { acquireChat } from './chat-registry';
	import { desktopChatPlatform } from './chat-platform';
	import ChatResumePicker from './ChatResumePicker.svelte';
	import PanelResizeHandle from './PanelResizeHandle.svelte';
	import { tasksPanelWidth } from './panel-width.svelte';

	let {
		agent,
		paneId,
		sessionId,
		project,
		cwd,
		claudeAccountId,
		onShowTerminal,
		onSessionIdChange
	}: {
		agent: AgentKind;
		paneId: string;
		/**
		 * Fixed for this component's life — the parent re-keys on a new session.
		 * Absent for a Codex pane with no thread yet: chat starts one.
		 */
		sessionId?: string;
		project: ProjectConfig;
		cwd?: string;
		/** The pane's Claude account; chat runs under the same login as its terminal. */
		claudeAccountId?: string;
		onShowTerminal: () => void;
		/** The chat got its session id (a new Codex thread) or moved to a new one (`/clear`). */
		onSessionIdChange: (sessionId: string) => void;
	} = $props();

	setChatPlatform(desktopChatPlatform);

	const workdir = $derived(cwd ?? project.path);
	const claudeSessionStore = getClaudeSessionStore();
	const settingsStore = getWorkbenchSettingsStore();
	const workspaceStore = getWorkspaceStore();
	const githubStore = getGitHubStore();
	// svelte-ignore state_referenced_locally
	const agentLabel = agentName(agent);
	const codexMode = codexChatMode(
		settingsStore.codexApprovalPolicy,
		settingsStore.codexSandboxMode
	);
	// svelte-ignore state_referenced_locally
	const { chat } = acquireChat(paneId, {
		agent,
		projectPath: project.path,
		...(cwd && cwd !== project.path ? { worktreePath: cwd } : {}),
		...(sessionId ? { sessionId } : {}),
		paneId,
		// The same launch settings terminal sessions get from Settings.
		...(agent === 'codex'
			? codexMode && { codexMode }
			: {
					...(claudeAccountId ? { claudeAccountId } : {}),
					...(settingsStore.claudePermissionMode !== 'default'
						? { permissionMode: settingsStore.claudePermissionMode }
						: {})
				}),
		// Another device's chat, or this pane's own terminal `claude`: join its
		// process, never start one behind its back.
		...(workspaceStore.isAdoptedPane(paneId) || workspaceStore.isLiveTerminalPane(paneId)
			? { attachOnly: true }
			: {})
	});
	const workspace = $derived(
		cwd && cwd !== project.path
			? workspaceStore.getByWorktreePath(cwd)
			: workspaceStore.getByProjectPath(project.path)
	);
	const branch = $derived(workspace && workspaceStore.resolvedBranch(workspace));
	const pr = $derived(branch ? githubStore.getBranchStatus(project.path, branch)?.pr : null);

	chat.onTakeOver = () => workspaceStore.takeOverPane(paneId);
	chat.onEnded = () => workspaceStore.closeEndedChat(paneId);
	chat.onTerminal = (terminalId) => workspaceStore.linkLiveTerminal(paneId, terminalId);

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
		const all = await claudeSessionStore.peekSessions(workdir, agent);
		return all
			.filter(
				(s) =>
					s.sessionId !== chat.sessionId &&
					(agent === 'codex' || (s.accountId ?? '') === (claudeAccountId ?? ''))
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
	let artifactsOpen = $state(false);
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
	const artifacts = $derived(chatArtifacts(chat.meta?.artifacts));
	const now = $derived(activity(chat.items, chat.meta));
	const live = $derived(chat.status === 'live');
	const suggestions = $derived(promptSuggestions(chat.meta, live && !chat.rewind, draft));
	// Codex reports its limits in the stream; Claude's come from the server's `/usage` check.
	// svelte-ignore state_referenced_locally
	const planLimits =
		agent === 'claude'
			? usePlanUsage(
					`loopback|${claudeAccountId ?? ''}`,
					(fresh) => planUsage(claudeAccountId, fresh),
					() => chat.meta
				)
			: null;
	const chips = $derived(planLimits?.chips ?? metaUsageChips(chat.meta));
	const disabledReason = $derived.by(() => {
		switch (chat.status) {
			case 'starting':
				return `Starting ${agentLabel}…`;
			case 'reconnecting':
				return 'Reconnecting…';
			case 'exited':
			case 'failed':
				return 'Restart the session to send messages';
			default:
				return asking ? `Answer ${agentLabel} above first` : null;
		}
	});
	const asking = $derived(now.kind === 'approval' || now.kind === 'elicitation');

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

	function send(text: string, images: ChatImage[], files: ChatFile[]): boolean {
		stickToBottom = true;
		return chat.prompt(text, images, files);
	}

	function onKeydown(event: KeyboardEvent) {
		if (event.key === 'Escape' && chat.meta?.busy && !asking) {
			event.preventDefault();
			chat.interrupt();
		}
	}
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
	{@attach measureWidth}
	class="relative flex h-full min-h-0 flex-col bg-wb-bg text-sm text-wb-ink"
	style:--wb-agent={agent === 'codex' ? 'var(--wb-codex)' : undefined}
	onkeydown={onKeydown}
>
	<header class="flex h-9 shrink-0 items-center gap-3 border-b border-wb-hair pr-40 pl-4 text-xs">
		<span
			class={cn(
				'size-1.5 shrink-0 rounded-full',
				live && (agent === 'codex' ? 'bg-wb-codex' : 'bg-wb-claude'),
				(chat.status === 'starting' || chat.status === 'reconnecting') &&
					'animate-pulse bg-wb-warn',
				(chat.status === 'exited' || chat.status === 'failed') && 'bg-wb-ink-soft'
			)}
		></span>
		<span class="min-w-0 truncate font-medium">{chat.meta?.title ?? agentLabel}</span>
		{#if chat.meta?.model}
			<span class="shrink-0 text-wb-ink-soft">{chat.meta.model.replace(/\[1m\]$/, '')}</span>
		{/if}
		{#if branch}
			<span class="flex min-w-0 items-center gap-1 font-mono text-[11px] text-wb-ink-mute">
				<GitBranchIcon class="size-3 shrink-0" />
				<span class="max-w-48 truncate" title={branch}>{branch}</span>
			</span>
		{/if}
		{#if pr}
			<PRStatusBadge {pr} onClickPr={() => openUrl(pr.url)} />
		{/if}
		<div class="ml-auto flex shrink-0 items-center gap-3">
			<ChatUsage {chips} chipClass="h-6 px-2 text-[11px]" />
			<ChatContext meta={chat.meta} class="h-6 px-2 text-[11px]" />
			<ChatCache {chat} class="h-6 px-2 text-[11px]" />
			{#if artifacts.length > 0}
				<button
					type="button"
					class="flex shrink-0 items-center gap-1.5 rounded-md px-2 py-0.5 text-wb-ink-mute hover:bg-wb-panel2 focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none"
					aria-expanded={artifactsOpen}
					onclick={() => (artifactsOpen = !artifactsOpen)}
				>
					<PanelsTopLeftIcon class="size-3.5" />
					{artifacts.length === 1 ? '1 artifact' : `${artifacts.length} artifacts`}
				</button>
			{/if}
			{#if tasks.length > 0 && !wide}
				<button
					type="button"
					class={cn(
						'flex shrink-0 items-center gap-1.5 rounded-md px-2 py-0.5 hover:bg-wb-panel2 focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none',
						runningTasks > 0
							? agent === 'codex'
								? 'text-wb-codex'
								: 'text-wb-claude'
							: 'text-wb-ink-mute'
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
					<span class="sr-only">Reconnecting to {agentLabel}</span>
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
				<ChatCacheHint {chat} />
				<ChatSuggestions {suggestions} onPick={(text) => (draft = text)} />
				<ChatComposer
					id="chat-draft-{paneId}"
					{agent}
					bind:draft
					mode={chat.meta?.permissionMode ?? null}
					busy={Boolean(chat.meta?.busy) && live}
					{disabledReason}
					onSend={send}
					onStop={() => chat.interrupt()}
					commands={commandList}
					onCommand={clientCommand}
					loadFiles={() => chat.listFiles()}
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
			<!-- The saved width is shared by every pane; a narrower pane keeps half for the chat. -->
			<div class="relative flex max-w-1/2 shrink-0" style:width="{tasksPanelWidth.width}px">
				<PanelResizeHandle size={tasksPanelWidth} label="Resize agents and tasks panel" />
				<ChatTasks
					{tasks}
					seenAt={chat.seenAt}
					cwd={workdir}
					fetchOutput={(id) => chat.taskOutput(id)}
					fetchTranscript={(id) => chat.taskTranscript(id)}
					class="w-full"
				/>
			</div>
		{/if}
	</div>

	{#if artifactsOpen && artifacts.length > 0}
		<div class="absolute inset-y-0 right-0 z-30 flex shadow-2xl">
			<ChatArtifacts {artifacts} onClose={() => (artifactsOpen = false)} />
		</div>
	{/if}

	{#if tasksOpen && tasks.length > 0 && !wide}
		<div class="absolute inset-y-0 right-0 z-20 flex shadow-2xl">
			<ChatTasks
				{tasks}
				seenAt={chat.seenAt}
				cwd={workdir}
				fetchOutput={(id) => chat.taskOutput(id)}
				fetchTranscript={(id) => chat.taskTranscript(id)}
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
