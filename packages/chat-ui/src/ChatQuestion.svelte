<script lang="ts">
	import { AgentIcon } from '@workbench/ui/agent-icon';
	import CheckIcon from '@lucide/svelte/icons/check';
	import XIcon from '@lucide/svelte/icons/x';
	import { cn } from '@workbench/ui';
	import type { AgentKind, ApprovalDecision } from '@workbench/types';
	import { agentName, answerFor, type ApprovalItem, parseQuestions } from './chat-format';

	let {
		approval,
		agent = 'claude',
		onAnswer
	}: {
		approval: ApprovalItem;
		agent?: AgentKind;
		onAnswer: (decision: ApprovalDecision, answers?: Record<string, string>) => void;
	} = $props();

	const name = $derived(agentName(agent));
	const questions = $derived(parseQuestions(approval.input));
	const resolved = $derived(Boolean(approval.decision) || approval.expired);

	/** Per question: chosen labels, the person's own words, the option being looked at. */
	let picks = $state<Record<number, string[]>>({});
	let others = $state<Record<number, string>>({});
	let looking = $state<Record<number, string>>({});
	let sent = $state(false);
	/** The question on screen; with several, the headers act as tabs. */
	let active = $state(0);

	const answers = $derived(
		Object.fromEntries(
			questions.map((q, i) => [q.id ?? q.question, answerFor(picks[i] ?? [], others[i] ?? '')])
		)
	);
	const complete = $derived(questions.every((q) => answers[q.id ?? q.question]));

	function pick(index: number, label: string, multi: boolean) {
		const current = picks[index] ?? [];
		if (multi) {
			picks[index] = current.includes(label)
				? current.filter((l) => l !== label)
				: [...current, label];
		} else {
			picks[index] = [label];
			others[index] = '';
			// A single choice is complete: move on to the next unanswered question.
			const next = questions.findIndex((q, i) => i > index && !answers[q.id ?? q.question]);
			if (next !== -1) active = next;
		}
		looking[index] = label;
	}

	function typeOther(index: number, multi: boolean, text: string) {
		others[index] = text;
		if (!multi && text) picks[index] = [];
	}

	function submit() {
		sent = true;
		onAnswer('allow', answers);
	}
</script>

{#if resolved}
	<div class="flex flex-col gap-1 text-xs text-wb-ink-soft">
		{#if approval.expired || approval.decision === 'deny'}
			<span class="flex items-center gap-2">
				<XIcon class="size-3.5" />
				{approval.expired ? `Question withdrawn by ${name}` : 'You skipped the question'}
			</span>
		{:else}
			{#each questions as q (q.id ?? q.question)}
				<span class="flex min-w-0 items-center gap-2">
					<CheckIcon class="size-3.5 shrink-0 text-wb-ok" />
					<span class="shrink-0">{q.header || q.question}</span>
					<span class="min-w-0 truncate text-wb-ink"
						>{q.isSecret ? 'Answered' : (approval.answers?.[q.id ?? q.question] ?? '')}</span
					>
				</span>
			{/each}
		{/if}
	</div>
{:else}
	<section
		class="question overflow-hidden rounded-lg border border-wb-accent/40 bg-wb-panel"
		aria-label="{name} has a question"
	>
		<header class="flex items-center gap-2 px-3.5 pt-3 text-xs text-wb-ink-mute">
			<AgentIcon {agent} class="size-4" />
			{approval.input?.isBlocking === false
				? `${name} has an optional question`
				: `${name} needs your input`}
		</header>
		{#if questions.length > 1}
			<div class="flex flex-wrap gap-1 px-3.5 pt-3" role="tablist" aria-label="Questions">
				{#each questions as q, qi (q.id ?? q.question)}
					{@const done = Boolean(answers[q.id ?? q.question])}
					<button
						type="button"
						role="tab"
						aria-selected={qi === active}
						class={cn(
							'flex items-center gap-1.5 rounded-md px-2.5 py-1 text-xs transition-colors focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none',
							qi === active
								? 'bg-wb-accent-soft text-wb-ink'
								: 'text-wb-ink-mute hover:bg-wb-panel2 hover:text-wb-ink'
						)}
						onclick={() => (active = qi)}
					>
						{#if done}
							<CheckIcon class="size-3 text-wb-ok" />
						{:else}
							<span class="size-1.5 rounded-full bg-wb-ink-soft"></span>
						{/if}
						{q.header || `Question ${qi + 1}`}
					</button>
				{/each}
			</div>
		{/if}
		<div class="flex flex-col gap-5 px-3.5 pt-3">
			{#each questions as q, qi (q.id ?? q.question)}
				{#if qi === active}
					{@const chosen = picks[qi] ?? []}
					{@const preview = q.options.find((o) => o.label === looking[qi])?.preview}
					<fieldset class="flex flex-col gap-2.5">
						<legend class="flex flex-wrap items-baseline gap-2">
							{#if q.header && questions.length === 1}
								<span class="rounded bg-wb-accent-soft px-1.5 py-0.5 text-[11px] text-wb-ink"
									>{q.header}</span
								>
							{/if}
							<span class="text-sm font-medium text-wb-ink">{q.question}</span>
							{#if q.multiSelect}
								<span class="text-[11px] text-wb-ink-soft">Choose any</span>
							{/if}
						</legend>
						<div
							class="grid gap-2 sm:grid-cols-2"
							role={q.multiSelect ? 'group' : 'radiogroup'}
							aria-label={q.question}
						>
							{#each q.options as option (option.label)}
								{@const on = chosen.includes(option.label)}
								<button
									type="button"
									role={q.multiSelect ? 'checkbox' : 'radio'}
									aria-checked={on}
									class={cn(
										'option flex items-start gap-2.5 rounded-md border px-3 py-2 text-left transition-colors focus-visible:ring-2 focus-visible:ring-wb-accent/50 focus-visible:outline-none',
										on
											? 'border-wb-accent bg-wb-accent-soft'
											: 'border-wb-hair hover:border-wb-ink-soft'
									)}
									onclick={() => pick(qi, option.label, q.multiSelect)}
									onmouseenter={() => (looking[qi] = option.label)}
									onfocus={() => (looking[qi] = option.label)}
								>
									<span
										class={cn(
											'mt-0.5 flex size-3.5 shrink-0 items-center justify-center border',
											q.multiSelect ? 'rounded-[3px]' : 'rounded-full',
											on ? 'border-wb-accent bg-wb-accent text-wb-accent-ink' : 'border-wb-ink-soft'
										)}
									>
										{#if on}<CheckIcon class="size-2.5" />{/if}
									</span>
									<span class="flex min-w-0 flex-col gap-0.5">
										<span class="text-xs font-medium text-wb-ink">{option.label}</span>
										{#if option.description}
											<span class="text-[11px] leading-snug text-wb-ink-mute"
												>{option.description}</span
											>
										{/if}
									</span>
								</button>
							{/each}
						</div>
						{#if preview}
							<pre
								class="scrollbar-thin max-h-48 overflow-auto rounded-md border border-wb-hair bg-wb-bg px-3 py-2 font-mono text-[11px] leading-relaxed whitespace-pre text-wb-ink-mute">{preview}</pre>
						{/if}
						{#if q.isOther !== false || q.options.length === 0}
							<label class="flex items-center gap-2 text-xs text-wb-ink-mute">
								<span class="shrink-0">Other</span>
								<input
									type={q.isSecret ? 'password' : 'text'}
									autocomplete="off"
									value={others[qi] ?? ''}
									oninput={(e) => typeOther(qi, q.multiSelect, e.currentTarget.value)}
									placeholder="Type your own answer"
									class="min-w-0 flex-1 rounded-md border border-wb-hair bg-wb-bg px-2.5 py-1.5 text-xs text-wb-ink placeholder:text-wb-ink-soft focus:border-wb-ink-soft focus:outline-none"
								/>
							</label>
						{/if}
					</fieldset>
				{/if}
			{/each}
		</div>
		<div class="flex items-center gap-2 px-3.5 py-3">
			<button
				type="button"
				class="rounded-md bg-wb-accent px-3 py-1 text-xs font-semibold text-wb-accent-ink hover:brightness-110 focus-visible:ring-2 focus-visible:ring-wb-accent/50 focus-visible:outline-none disabled:opacity-40"
				disabled={!complete || sent}
				onclick={submit}
			>
				{sent ? 'Sending…' : questions.length > 1 ? 'Send answers' : 'Send answer'}
			</button>
			{#if !complete && !sent}
				<span class="text-[11px] text-wb-ink-soft">
					{questions.length > 1
						? `${questions.filter((q) => answers[q.id ?? q.question]).length} of ${questions.length} answered`
						: 'Choose an answer to continue'}
				</span>
			{/if}
			<button
				type="button"
				class="ml-auto rounded-md px-3 py-1 text-xs text-wb-ink-mute hover:text-wb-ink focus-visible:ring-2 focus-visible:ring-wb-accent/50 focus-visible:outline-none disabled:opacity-40"
				disabled={sent}
				onclick={() => {
					sent = true;
					onAnswer('deny');
				}}
			>
				Skip
			</button>
		</div>
	</section>
{/if}

<style>
	.question {
		animation: arrive 260ms cubic-bezier(0.2, 0.8, 0.2, 1);
		box-shadow: 0 0 0 3px color-mix(in oklab, var(--wb-accent) 10%, transparent);
	}
	@keyframes arrive {
		from {
			opacity: 0;
			transform: translateY(6px);
		}
	}
	.option:active {
		transform: scale(0.99);
	}
	@media (prefers-reduced-motion: reduce) {
		.question {
			animation: none;
		}
		.option:active {
			transform: none;
		}
	}
</style>
