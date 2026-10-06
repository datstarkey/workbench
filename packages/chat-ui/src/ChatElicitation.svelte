<script lang="ts">
	import CheckIcon from '@lucide/svelte/icons/check';
	import ExternalLinkIcon from '@lucide/svelte/icons/external-link';
	import PlugIcon from '@lucide/svelte/icons/plug';
	import XIcon from '@lucide/svelte/icons/x';
	import type { AgentKind, ElicitationAction } from '@workbench/types';
	import { agentName } from './chat-format';
	import {
		elicitationContent,
		elicitationFields,
		elicitationUrl,
		initialDraft,
		type ElicitationDraft,
		type ElicitationItem,
		type ElicitationValue
	} from './elicitation-form';
	import { getChatPlatform } from './platform';

	let {
		elicitation,
		agent = 'claude',
		onAnswer
	}: {
		elicitation: ElicitationItem;
		agent?: AgentKind;
		onAnswer: (
			action: ElicitationAction,
			content?: Record<string, ElicitationValue>
		) => void | Promise<void>;
	} = $props();

	const platform = getChatPlatform();
	const uid = $props.id();

	const fields = $derived(elicitationFields(elicitation.schema));
	/** Only what the person changed; the rest comes from the schema's defaults. */
	let edits = $state<ElicitationDraft>({});
	const draft = $derived({ ...initialDraft(elicitation.schema, fields), ...edits });
	const result = $derived(elicitationContent(fields, draft));
	/** Errors show once a send was tried, not while the form is first filled in. */
	let tried = $state(false);
	let serverError = $state('');
	let sent = $state<ElicitationAction | null>(null);

	const isUrl = $derived(elicitation.mode === 'url');
	const url = $derived(elicitationUrl(elicitation));
	const host = $derived(url ? new URL(url).host : null);
	const heading = $derived(elicitation.title ?? `${elicitation.server} needs your input`);

	const outcome = $derived.by(() => {
		if (elicitation.expired) return `${elicitation.server}'s request was withdrawn`;
		switch (elicitation.action) {
			case 'accept':
				if (!isUrl) return `Sent to ${elicitation.server}`;
				return elicitation.completed
					? `Finished with ${elicitation.server}`
					: `Waiting for ${elicitation.server} to finish in the browser`;
			case 'decline':
				return `You declined ${elicitation.server}'s request`;
			case 'cancel':
				return `You dismissed ${elicitation.server}'s request`;
			default:
				return null;
		}
	});
	const sentSummary = $derived(
		fields
			.filter((f) => elicitation.content?.[f.name] !== undefined)
			.map((f) => {
				const v = elicitation.content?.[f.name];
				return `${f.label}: ${Array.isArray(v) ? v.join(', ') : String(v)}`;
			})
			.join(' · ')
	);

	async function answer(action: ElicitationAction, content?: Record<string, ElicitationValue>) {
		sent = action;
		serverError = '';
		try {
			await onAnswer(action, content);
		} catch (e) {
			serverError = e instanceof Error ? e.message : String(e);
			sent = null;
		}
	}

	function submit(event: SubmitEvent) {
		event.preventDefault();
		tried = true;
		if (Object.keys(result.errors).length === 0) answer('accept', result.content);
	}

	function open() {
		if (!url) return;
		platform.openLink(url);
		answer('accept');
	}

	const text = (name: string) => {
		const v = draft[name];
		return typeof v === 'string' ? v : '';
	};
	const picked = (name: string) => {
		const v = draft[name];
		return Array.isArray(v) ? v : [];
	};

	function toggle(name: string, value: string, on: boolean) {
		const current = picked(name);
		edits[name] = on ? [...current, value] : current.filter((v) => v !== value);
	}

	const inputClass =
		'min-w-0 rounded-md border border-wb-hair bg-wb-bg px-2.5 py-1.5 text-xs text-wb-ink placeholder:text-wb-ink-soft focus:border-wb-ink-soft focus:outline-none';
	const textTypes: Record<string, string> = {
		email: 'email',
		uri: 'url',
		date: 'date',
		'date-time': 'datetime-local'
	};
</script>

{#if outcome}
	<div class="flex min-w-0 items-center gap-2 text-xs text-wb-ink-soft">
		{#if elicitation.action === 'accept'}
			<CheckIcon class="size-3.5 shrink-0 text-wb-ok" />
		{:else}
			<XIcon class="size-3.5 shrink-0" />
		{/if}
		<span class="shrink-0">{outcome}</span>
		{#if sentSummary}
			<span class="min-w-0 truncate text-wb-ink">{sentSummary}</span>
		{/if}
	</div>
{:else}
	<section
		class="elicitation overflow-hidden rounded-lg border border-wb-accent/40 bg-wb-panel"
		aria-label="{elicitation.server} needs your input"
	>
		<header class="flex items-center gap-2 px-3.5 pt-3 text-sm">
			<PlugIcon class="size-4 shrink-0 text-wb-accent" />
			<span class="font-medium text-wb-ink">{heading}</span>
			<span class="ml-auto shrink-0 text-xs text-wb-ink-soft">MCP · {agentName(agent)}</span>
		</header>
		{#if elicitation.message}
			<p class="px-3.5 pt-2 text-sm leading-relaxed whitespace-pre-wrap text-wb-ink">
				{elicitation.message}
			</p>
		{/if}
		{#if elicitation.description}
			<p class="px-3.5 pt-1 text-xs text-wb-ink-mute">{elicitation.description}</p>
		{/if}

		{#if isUrl}
			<div class="flex flex-col gap-1 px-3.5 pt-2.5 text-xs">
				{#if url}
					<span class="text-wb-ink-mute"
						>Opens <span class="font-mono text-wb-ink">{host}</span></span
					>
					<span class="truncate font-mono text-[11px] text-wb-ink-soft">{url}</span>
				{:else}
					<span class="text-wb-err">This link can't be opened: it isn't a web address.</span>
				{/if}
			</div>
			<div class="flex flex-wrap items-center gap-2 px-3.5 py-3">
				{#if url}
					<button
						type="button"
						class="flex items-center gap-1.5 rounded-md bg-wb-accent px-3 py-1 text-xs font-semibold text-wb-accent-ink hover:brightness-110 focus-visible:ring-2 focus-visible:ring-wb-accent/50 focus-visible:outline-none disabled:opacity-50"
						disabled={sent !== null}
						onclick={open}
					>
						<ExternalLinkIcon class="size-3.5" />
						{sent === 'accept' ? 'Opening…' : `Open ${host}`}
					</button>
				{/if}
				{@render dismiss()}
			</div>
		{:else}
			<form class="flex flex-col gap-3 px-3.5 pt-3" novalidate onsubmit={submit}>
				{#each fields as f (f.name)}
					{@const id = `${uid}-${f.name}`}
					{@const error = tried ? result.errors[f.name] : undefined}
					<div class="flex flex-col gap-1">
						{#if f.type === 'boolean'}
							<label class="flex items-center gap-2 text-xs text-wb-ink">
								<input
									{id}
									type="checkbox"
									class="size-3.5 accent-wb-accent"
									checked={draft[f.name] === true}
									onchange={(e) => (edits[f.name] = e.currentTarget.checked)}
								/>
								{f.label}
							</label>
						{:else if f.type === 'multiselect'}
							<fieldset class="flex flex-col gap-1">
								<legend class="pb-1 text-xs font-medium text-wb-ink"
									>{f.label}{f.required ? ' *' : ''}</legend
								>
								{#each f.options as o (o.value)}
									<label class="flex items-center gap-2 text-xs text-wb-ink">
										<input
											type="checkbox"
											class="size-3.5 accent-wb-accent"
											checked={picked(f.name).includes(o.value)}
											onchange={(e) => toggle(f.name, o.value, e.currentTarget.checked)}
										/>
										{o.label}
									</label>
								{/each}
							</fieldset>
						{:else}
							<label for={id} class="text-xs font-medium text-wb-ink"
								>{f.label}{f.required ? ' *' : ''}</label
							>
							{#if f.type === 'select'}
								<select
									{id}
									class={inputClass}
									value={text(f.name)}
									onchange={(e) => (edits[f.name] = e.currentTarget.value)}
								>
									<option value="">Choose…</option>
									{#each f.options as o (o.value)}
										<option value={o.value}>{o.label}</option>
									{/each}
								</select>
							{:else}
								<input
									{id}
									type={f.type === 'string' ? (textTypes[f.format ?? ''] ?? 'text') : 'number'}
									step={f.type === 'number' ? 'any' : undefined}
									min={f.min}
									max={f.max}
									class={inputClass}
									value={text(f.name)}
									aria-invalid={error ? true : undefined}
									oninput={(e) => (edits[f.name] = e.currentTarget.value)}
								/>
							{/if}
						{/if}
						{#if f.description}
							<span class="text-[11px] text-wb-ink-mute">{f.description}</span>
						{/if}
						{#if error}
							<span class="text-[11px] text-wb-err" role="alert">{error}</span>
						{/if}
					</div>
				{/each}
				<div class="flex flex-wrap items-center gap-2 pb-3">
					<button
						type="submit"
						class="rounded-md bg-wb-accent px-3 py-1 text-xs font-semibold text-wb-accent-ink hover:brightness-110 focus-visible:ring-2 focus-visible:ring-wb-accent/50 focus-visible:outline-none disabled:opacity-50"
						disabled={sent !== null}
					>
						{sent === 'accept' ? 'Sending…' : 'Send'}
					</button>
					{@render dismiss()}
				</div>
			</form>
		{/if}
		{#if serverError}<p role="alert" class="px-3.5 pb-3 text-xs text-wb-err">{serverError}</p>{/if}
	</section>
{/if}

{#snippet dismiss()}
	<button
		type="button"
		class="ml-auto rounded-md px-3 py-1 text-xs text-wb-ink-mute hover:text-wb-err focus-visible:ring-2 focus-visible:ring-wb-accent/50 focus-visible:outline-none disabled:opacity-50"
		disabled={sent !== null}
		onclick={() => answer('decline')}
	>
		Decline
	</button>
	<button
		type="button"
		class="rounded-md px-3 py-1 text-xs text-wb-ink-mute hover:text-wb-ink focus-visible:ring-2 focus-visible:ring-wb-accent/50 focus-visible:outline-none disabled:opacity-50"
		disabled={sent !== null}
		onclick={() => answer('cancel')}
	>
		Not now
	</button>
{/snippet}

<style>
	.elicitation {
		animation: arrive 260ms cubic-bezier(0.2, 0.8, 0.2, 1);
		box-shadow: 0 0 0 3px color-mix(in oklab, var(--wb-accent) 10%, transparent);
	}
	@keyframes arrive {
		from {
			opacity: 0;
			transform: translateY(6px);
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.elicitation {
			animation: none;
		}
	}
</style>
