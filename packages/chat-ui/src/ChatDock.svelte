<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { SlashCommand } from '@workbench/types';
	import { cn } from '@workbench/ui';
	import type { AgentChat } from './agent-chat.svelte';
	import { agentName, limitNotice } from './chat-format';
	import { promptSuggestions } from './artifacts';
	import ChatCacheHint from './ChatCacheHint.svelte';
	import ChatComposer from './ChatComposer.svelte';
	import ChatModelPicker from './ChatModelPicker.svelte';
	import ChatPlan from './ChatPlan.svelte';
	import ChatSuggestions from './ChatSuggestions.svelte';
	import CodexControls from './CodexControls.svelte';

	let {
		chat,
		id,
		notice,
		answerHint,
		onResume,
		onThread,
		popover,
		class: className
	}: {
		chat: AgentChat;
		/** The composer's element id. */
		id: string;
		/** A host error shown in place of the chat's own notice. */
		notice?: string | null;
		/** The composer's placeholder while a request waits on the person; says where to answer it. */
		answerHint?: string;
		/** `/resume` is a terminal picker the CLI doesn't offer in chat, so the host provides it. */
		onResume: () => void;
		/** Open another Codex thread (CodexControls' thread list). */
		onThread: (sessionId: string, label: string) => void;
		/** Shown above the composer, e.g. the resume picker. */
		popover?: Snippet;
		class?: string;
	} = $props();

	const RESUME: SlashCommand = {
		name: 'resume',
		description: 'Continue an earlier conversation from this folder'
	};

	const name = $derived(agentName(chat.agent));
	const shownNotice = $derived(notice ?? chat.notice);
	const limit = $derived(limitNotice(chat.meta?.rateLimit ?? null));
	const suggestions = $derived(
		promptSuggestions(chat.meta, chat.live && !chat.rewind, chat.draft.text)
	);
	const commands = $derived([RESUME, ...chat.commands.filter((c) => c.name !== 'resume')]);
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
				return chat.waiting ? (answerHint ?? `Answer ${name} first`) : null;
		}
	});

	function onCommand(command: string): boolean {
		if (command !== 'resume') return false;
		onResume();
		return true;
	}
</script>

<div class={cn('flex flex-col gap-2', className)}>
	{#if shownNotice}
		<p class="text-xs text-wb-err" role="alert">{shownNotice}</p>
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
	{#if chat.todos.length > 0}
		<ChatPlan steps={chat.todos} />
	{/if}
	<CodexControls {chat} {onThread} />
	<ChatCacheHint {chat} />
	<ChatSuggestions {suggestions} onPick={(text) => (chat.draft.text = text)} />
	<ChatComposer
		{id}
		agent={chat.agent}
		bind:draft={chat.draft.text}
		bind:images={chat.draft.images}
		bind:files={chat.draft.files}
		mode={chat.meta?.permissionMode ?? null}
		busy={Boolean(chat.meta?.busy) && chat.live}
		stoppable={chat.stoppable}
		{disabledReason}
		onSend={(text, images, files) => chat.prompt(text, images, files)}
		onStop={() => chat.interrupt()}
		{commands}
		{onCommand}
		loadFiles={() => chat.listFiles()}
		onMode={(mode) => chat.setMode(mode)}
		{popover}
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
