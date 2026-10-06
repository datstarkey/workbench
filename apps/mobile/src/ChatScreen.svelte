<script lang="ts">
	import { AgentIcon } from '@workbench/ui/agent-icon';
	import { onDestroy, onMount } from 'svelte';
	import { watch } from 'runed';
	import ChevronLeftIcon from '@lucide/svelte/icons/chevron-left';
	import EllipsisVerticalIcon from '@lucide/svelte/icons/ellipsis-vertical';
	import PanelsTopLeftIcon from '@lucide/svelte/icons/panels-top-left';
	import {
		AgentChat,
		agentName,
		ChatAnswer,
		ChatArtifacts,
		ChatCache,
		ChatContext,
		ChatDock,
		ChatTasks,
		ChatTranscript,
		ChatUsage,
		contextUsed,
		followLatest,
		metaUsageChips,
		setChatPlatform,
		usePlanUsage
	} from '@workbench/chat-ui';
	import { cn } from '@workbench/ui';
	import * as DropdownMenu from '@workbench/ui/dropdown-menu';
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
	const drafts = client.drafts;
	// svelte-ignore state_referenced_locally
	const chat = new AgentChat(
		{
			...(ref.agent === 'codex' ? { agent: 'codex' as const } : {}),
			projectPath: ref.projectPath,
			...(ref.worktreePath ? { worktreePath: ref.worktreePath } : {}),
			...(ref.sessionId ? { sessionId: ref.sessionId } : {}),
			...(ref.attachOnly ? { attachOnly: true } : {}),
			...(ref.claudeAccountId ? { claudeAccountId: ref.claudeAccountId } : {})
		},
		client.agents,
		{ draft: drafts.get(ref), reconnectOnWake: true }
	);
	const draft = chat.draft;
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
	const tasks = $derived(chat.tasks);
	const artifacts = $derived(chat.artifactList);
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
	const waiting = $derived(chat.waiting);
	/** The sheet can be lowered to read the chat behind it; a new request raises it again. */
	let sheetHiddenFor = $state<string | null>(null);
	const sheetOpen = $derived(waiting !== null && sheetHiddenFor !== waiting.id && chat.live);
	let tasksOpen = $state(false);
	let artifactsOpen = $state(false);
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

	/** `/clear` may have moved the conversation to a new id since this screen opened. */
	function showTerminal() {
		void client.showAsTerminal({ ...ref, sessionId: chat.sessionId });
	}

	// The session keeps running on the server; leaving only detaches.
	onDestroy(() => {
		drafts.save(ref, draft);
		chat.dispose();
	});
</script>

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
		<AgentIcon agent={chat.agent} class="size-5" />
		<div class="flex min-w-0 flex-1 flex-col">
			<span class="truncate text-[14px] font-semibold">{chat.title}</span>
			<span class="flex items-center gap-1.5 truncate font-mono text-[10.5px] text-wb-ink-soft">
				<span
					class={cn(
						'size-1.5 shrink-0 rounded-full',
						chat.live && (waiting ? 'bg-wb-warn' : chat.meta?.busy ? 'bg-wb-accent' : 'bg-wb-ok'),
						(chat.status === 'starting' || chat.status === 'reconnecting') &&
							'animate-pulse bg-wb-warn',
						(chat.status === 'exited' || chat.status === 'failed') && 'bg-wb-ink-soft'
					)}
				></span>
				{place}
			</span>
		</div>
		{#if isClaude}
			<ViewSwitch view="chat" disabled={client.switching || !chat.live} onSwitch={showTerminal} />
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
				<DropdownMenu.Item onSelect={() => chat.open()}>Reconnect</DropdownMenu.Item>
				<DropdownMenu.Item class="text-wb-err" onSelect={() => client.endChat(chat.sessionId)}>
					End session
				</DropdownMenu.Item>
			</DropdownMenu.Content>
		</DropdownMenu.Root>
	</header>

	{#if chat.status === 'reconnecting' || (chat.live && chat.meta?.busy)}
		<div
			class={cn(
				'bar relative h-0.5 shrink-0 overflow-hidden bg-wb-hair-soft',
				!chat.live && 'warn'
			)}
			role="status"
		>
			<span class="sr-only">{chat.live ? `${name} is working` : 'Reconnecting'}</span>
		</div>
	{/if}

	<div
		{@attach followLatest(chat)}
		class="min-h-0 flex-1 overflow-y-auto"
		role="log"
		aria-live="polite"
	>
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

	{#if waiting && !sheetOpen && chat.live}
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
						chat.runningTasks > 0
							? isClaude
								? 'text-wb-claude'
								: 'text-wb-codex'
							: 'text-wb-ink-mute'
					)}
					onclick={() => (tasksOpen = true)}
				>
					<AgentIcon agent={chat.agent} class="size-3.5" />
					{chat.runningTasks > 0 ? `${chat.runningTasks} running` : `${tasks.length} tasks`}
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
		class="shrink-0 border-t border-wb-hair bg-wb-rail px-2.5 pt-2"
		style="padding-bottom: calc(0.5rem + env(safe-area-inset-bottom));"
	>
		<ChatDock
			{chat}
			id="chat-draft-{ref.sessionId}"
			notice={client.notice}
			onResume={() => (reviewOpen = 'history')}
			onThread={(sessionId, name) => client.openChat({ ...ref, sessionId, name, agent: 'codex' })}
		/>
	</div>
</div>

{#if sheetOpen && waiting}
	<Sheet label="{name} is waiting on you" onClose={() => (sheetHiddenFor = waiting.id)}>
		<ChatAnswer item={waiting} {chat} {cwd} />
	</Sheet>
{/if}

{#if tasksOpen && tasks.length > 0}
	<Sheet label="Agents and tasks" onClose={() => (tasksOpen = false)}>
		<ChatTasks
			agent={chat.agent}
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
