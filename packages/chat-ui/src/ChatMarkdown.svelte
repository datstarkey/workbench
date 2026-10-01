<script lang="ts" module>
	import type { Token, Tokens } from 'marked';

	/** Links that may leave the app; anything else renders as plain text. */
	export function safeHref(href: string): string | null {
		return /^(https?:|mailto:)/i.test(href.trim()) ? href.trim() : null;
	}
</script>

<script lang="ts">
	import { marked } from 'marked';
	import CheckIcon from '@lucide/svelte/icons/check';
	import CopyIcon from '@lucide/svelte/icons/copy';
	import { cn } from '@workbench/ui';
	import { getChatPlatform } from './platform';

	/**
	 * Claude's Markdown, rendered from `marked`'s tokens as Svelte elements —
	 * never `{@html}`: replies can quote untrusted repo content and this webview
	 * can call into the app. Raw HTML in the text is shown as text.
	 */
	let { text }: { text: string } = $props();

	const tokens = $derived(marked.lexer(text, { gfm: true }));

	let copied = $state<string | null>(null);

	async function copy(code: string) {
		try {
			await navigator.clipboard.writeText(code);
			copied = code;
			setTimeout(() => (copied = copied === code ? null : copied), 1500);
		} catch {
			// Clipboard refused: leave the code selectable.
		}
	}

	const platform = getChatPlatform();

	function openLink(event: MouseEvent, href: string) {
		event.preventDefault();
		platform.openLink(href);
	}

	const HEADING = [
		'',
		'text-lg font-semibold',
		'text-base font-semibold',
		'text-sm font-semibold',
		'text-sm font-medium',
		'text-sm font-medium',
		'text-sm font-medium'
	];
</script>

{#snippet inline(list: Token[] | undefined)}
	{#each list ?? [] as token, i (i)}
		{#if token.type === 'strong'}
			<strong class="font-semibold text-wb-ink"
				>{@render inline((token as Tokens.Strong).tokens)}</strong
			>
		{:else if token.type === 'em'}
			<em>{@render inline((token as Tokens.Em).tokens)}</em>
		{:else if token.type === 'del'}
			<del class="text-wb-ink-mute">{@render inline((token as Tokens.Del).tokens)}</del>
		{:else if token.type === 'codespan'}
			<code class="rounded bg-wb-panel2 px-1 py-px font-mono text-[0.86em] break-words text-wb-ink"
				>{(token as Tokens.Codespan).text}</code
			>
		{:else if token.type === 'link'}
			{@const link = token as Tokens.Link}
			{@const href = safeHref(link.href)}
			{#if href}
				<a
					{href}
					title={link.title ?? href}
					class="text-wb-accent underline decoration-wb-accent/40 underline-offset-2 hover:decoration-wb-accent"
					onclick={(e) => openLink(e, href)}>{@render inline(link.tokens)}</a
				>
			{:else}
				{@render inline(link.tokens)}
			{/if}
		{:else if token.type === 'image'}
			{@const image = token as Tokens.Image}
			{@const href = safeHref(image.href)}
			<!-- Never load remote images: a reply could use one as a tracking beacon. -->
			{#if href}
				<a
					{href}
					class="text-wb-accent underline underline-offset-2"
					onclick={(e) => openLink(e, href)}>{image.text || 'image'}</a
				>
			{:else}
				{image.text}
			{/if}
		{:else if token.type === 'br'}
			<br />
		{:else if token.type === 'text' && 'tokens' in token && token.tokens}
			{@render inline(token.tokens)}
		{:else if 'text' in token}
			{token.text}
		{/if}
	{/each}
{/snippet}

{#snippet blocks(list: Token[])}
	{#each list as token, i (i)}
		{#if token.type === 'heading'}
			{@const heading = token as Tokens.Heading}
			<p
				class={cn('mt-1 text-wb-ink', HEADING[heading.depth])}
				role="heading"
				aria-level={heading.depth}
			>
				{@render inline(heading.tokens)}
			</p>
		{:else if token.type === 'paragraph'}
			<p>{@render inline((token as Tokens.Paragraph).tokens)}</p>
		{:else if token.type === 'text'}
			<p>{@render inline((token as Tokens.Text).tokens ?? [token])}</p>
		{:else if token.type === 'code'}
			{@const code = token as Tokens.Code}
			<div class="group/code relative overflow-hidden rounded-md border border-wb-hair bg-wb-panel">
				<div class="flex items-center justify-between border-b border-wb-hair px-3 py-1">
					<span class="font-mono text-[10px] text-wb-ink-soft">{code.lang || 'text'}</span>
					<button
						type="button"
						class="flex items-center gap-1 rounded px-1 text-[11px] text-wb-ink-soft hover:text-wb-ink focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none"
						aria-label="Copy code"
						onclick={() => copy(code.text)}
					>
						{#if copied === code.text}
							<CheckIcon class="size-3 text-wb-ok" /> Copied
						{:else}
							<CopyIcon class="size-3" /> Copy
						{/if}
					</button>
				</div>
				<pre
					class="scrollbar-thin overflow-x-auto px-3 py-2 font-mono text-xs leading-relaxed text-wb-ink">{code.text}</pre>
			</div>
		{:else if token.type === 'blockquote'}
			<blockquote class="flex flex-col gap-2 border-l-2 border-wb-hair pl-3 text-wb-ink-mute">
				{@render blocks((token as Tokens.Blockquote).tokens)}
			</blockquote>
		{:else if token.type === 'list'}
			{@const list = token as Tokens.List}
			<svelte:element
				this={list.ordered ? 'ol' : 'ul'}
				start={list.ordered && list.start !== '' ? Number(list.start) : undefined}
				class={cn(
					'flex flex-col gap-1 pl-5',
					list.ordered ? 'list-decimal' : 'list-disc',
					list.items.some((item) => item.task) && 'list-none pl-1'
				)}
			>
				{#each list.items as item, j (j)}
					<li class="pl-0.5 marker:text-wb-ink-soft">
						<div class={cn('flex gap-2', !item.task && 'contents')}>
							{#if item.task}
								<span
									class={cn(
										'mt-1 flex size-3 shrink-0 items-center justify-center rounded-[3px] border',
										item.checked ? 'border-wb-ok bg-wb-ok text-wb-accent-ink' : 'border-wb-ink-soft'
									)}
									aria-label={item.checked ? 'Done' : 'Not done'}
								>
									{#if item.checked}<CheckIcon class="size-2.5" />{/if}
								</span>
							{/if}
							<div class="flex min-w-0 flex-col gap-1">{@render blocks(item.tokens)}</div>
						</div>
					</li>
				{/each}
			</svelte:element>
		{:else if token.type === 'table'}
			{@const table = token as Tokens.Table}
			<div class="scrollbar-thin overflow-x-auto rounded-md border border-wb-hair">
				<table class="w-full border-collapse text-xs">
					<thead class="bg-wb-panel">
						<tr>
							{#each table.header as cell, c (c)}
								<th
									class="border-b border-wb-hair px-3 py-1.5 font-medium text-wb-ink"
									style:text-align={cell.align ?? 'left'}
								>
									{@render inline(cell.tokens)}
								</th>
							{/each}
						</tr>
					</thead>
					<tbody>
						{#each table.rows as row, r (r)}
							<tr class="border-b border-wb-hair last:border-0">
								{#each row as cell, c (c)}
									<td class="px-3 py-1.5 align-top" style:text-align={cell.align ?? 'left'}>
										{@render inline(cell.tokens)}
									</td>
								{/each}
							</tr>
						{/each}
					</tbody>
				</table>
			</div>
		{:else if token.type === 'hr'}
			<hr class="border-wb-hair" />
		{:else if token.type === 'html'}
			<pre class="font-mono text-xs whitespace-pre-wrap text-wb-ink-mute">{(token as Tokens.HTML)
					.text}</pre>
		{/if}
	{/each}
{/snippet}

<div class="markdown flex min-w-0 flex-col gap-2.5 leading-relaxed">
	{@render blocks(tokens)}
</div>
