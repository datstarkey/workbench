<script lang="ts">
	import type { AgentChat } from './agent-chat.svelte';
	import type { ApprovalItem } from './chat-format';
	import type { ElicitationItem } from './elicitation-form';
	import ChatApproval from './ChatApproval.svelte';
	import ChatElicitation from './ChatElicitation.svelte';
	import ChatQuestion from './ChatQuestion.svelte';

	let { item, chat, cwd }: { item: ApprovalItem | ElicitationItem; chat: AgentChat; cwd: string } =
		$props();
</script>

{#if item.kind === 'elicitation'}
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
