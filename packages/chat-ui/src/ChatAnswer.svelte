<script lang="ts">
	import SquareTerminalIcon from '@lucide/svelte/icons/square-terminal';
	import type { AgentChat } from './agent-chat.svelte';
	import { type ApprovalItem, terminalAnswer } from './chat-format';
	import type { ElicitationItem } from './elicitation-form';
	import ChatApproval from './ChatApproval.svelte';
	import ChatElicitation from './ChatElicitation.svelte';
	import ChatQuestion from './ChatQuestion.svelte';

	let {
		item,
		chat,
		cwd,
		onShowTerminal
	}: {
		item: ApprovalItem | ElicitationItem;
		chat: AgentChat;
		cwd: string;
		onShowTerminal?: () => void;
	} = $props();

	const inTerminal = $derived(item.kind === 'approval' ? terminalAnswer(item) : null);
</script>

{#if inTerminal}
	<p class="flex min-w-0 items-center gap-2 text-xs text-wb-ink-soft">
		<SquareTerminalIcon class="size-3.5 shrink-0" />
		<span class="min-w-0 truncate">
			{inTerminal === 'waiting'
				? 'Waiting for your answer in the terminal'
				: 'Answered in the terminal'}
		</span>
		{#if inTerminal === 'waiting' && onShowTerminal}
			<button type="button" class="shrink-0 underline hover:text-wb-ink" onclick={onShowTerminal}
				>Show terminal</button
			>
		{/if}
	</p>
{:else if item.kind === 'elicitation'}
	<ChatElicitation
		elicitation={item}
		agent={chat.agent}
		onAnswer={(action, content) => chat.elicit(item.id, action, content)}
	/>
{:else if item.tool === 'AskUserQuestion'}
	<ChatQuestion
		approval={item}
		agent={chat.agent}
		onAnswer={(decision, answers) => chat.approve(item.id, decision, answers)}
	/>
{:else}
	<ChatApproval
		approval={item}
		agent={chat.agent}
		{cwd}
		onDecide={(decision) => chat.approve(item.id, decision)}
	/>
{/if}
