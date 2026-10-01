<script lang="ts">
	import type { Attachment } from 'svelte/attachments';
	import { watch } from 'runed';
	import ArrowUpIcon from '@lucide/svelte/icons/arrow-up';
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import SquareIcon from '@lucide/svelte/icons/square';
	import { cn } from '@workbench/ui';
	import * as DropdownMenu from '@workbench/ui/dropdown-menu';
	import type { PermissionMode } from '$types/workbench';
	import { MODE_OPTIONS, modeLabel } from './chat-format';

	let {
		id,
		draft = $bindable(''),
		mode,
		busy,
		disabledReason,
		onSend,
		onStop,
		onMode
	}: {
		id: string;
		draft?: string;
		mode: PermissionMode | null;
		busy: boolean;
		/** Set when nothing can be sent right now; shown as the placeholder. */
		disabledReason: string | null;
		/** Returns false if the message could not be sent (the draft is kept). */
		onSend: (text: string) => boolean;
		onStop: () => void;
		onMode: (mode: PermissionMode) => void;
	} = $props();

	const canSend = $derived(!disabledReason && draft.trim().length > 0);

	/** Grow with the text up to ~8 lines, then scroll. */
	const autosize: Attachment<HTMLTextAreaElement> = (node) => {
		watch(
			() => draft,
			() => {
				node.style.height = 'auto';
				node.style.height = `${Math.min(node.scrollHeight, 180)}px`;
			}
		);
	};

	function send() {
		if (canSend && onSend(draft)) draft = '';
	}

	function onKeydown(event: KeyboardEvent) {
		if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) {
			event.preventDefault();
			send();
		}
	}
</script>

<div
	class="composer rounded-xl border border-wb-hair bg-wb-panel transition-colors focus-within:border-wb-ink-soft"
>
	<label for={id} class="sr-only">Message Claude</label>
	<textarea
		{id}
		bind:value={draft}
		{@attach autosize}
		onkeydown={onKeydown}
		rows="1"
		disabled={disabledReason !== null}
		placeholder={disabledReason ?? 'Message Claude'}
		class="block max-h-[180px] w-full resize-none bg-transparent px-3.5 pt-3 pb-1 text-sm leading-relaxed text-wb-ink placeholder:text-wb-ink-soft focus:outline-none disabled:cursor-not-allowed"
	></textarea>
	<div class="flex items-center gap-1.5 px-2 pb-2">
		<DropdownMenu.Root>
			<DropdownMenu.Trigger>
				{#snippet child({ props })}
					<button
						{...props}
						type="button"
						disabled={disabledReason !== null}
						class={cn(
							'flex items-center gap-1 rounded-md px-2 py-1 text-xs hover:bg-wb-panel2 focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none disabled:opacity-50',
							mode === 'bypassPermissions' ? 'text-wb-err' : 'text-wb-ink-mute'
						)}
						title="Permission mode"
					>
						{modeLabel(mode)}
						<ChevronDownIcon class="size-3" />
					</button>
				{/snippet}
			</DropdownMenu.Trigger>
			<DropdownMenu.Content align="start" class="w-64">
				<DropdownMenu.RadioGroup
					value={mode ?? 'default'}
					onValueChange={(value) => onMode(value as PermissionMode)}
				>
					{#each MODE_OPTIONS as option (option.mode)}
						<DropdownMenu.RadioItem value={option.mode} class="flex-col items-start gap-0">
							<span class={cn(option.mode === 'bypassPermissions' && 'text-wb-err')}>
								{option.label}
							</span>
							<span class="text-[11px] text-muted-foreground">{option.hint}</span>
						</DropdownMenu.RadioItem>
					{/each}
				</DropdownMenu.RadioGroup>
			</DropdownMenu.Content>
		</DropdownMenu.Root>
		<span class="flex-1"></span>
		{#if busy}
			<button
				type="button"
				class="flex size-7 items-center justify-center rounded-lg border border-wb-hair bg-wb-panel2 text-wb-ink-mute hover:text-wb-ink focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none"
				title="Stop (Esc)"
				aria-label="Stop"
				onclick={onStop}
			>
				<SquareIcon class="size-2.5 fill-current" />
			</button>
		{/if}
		<button
			type="button"
			class="flex size-7 items-center justify-center rounded-lg bg-wb-accent text-wb-accent-ink transition-opacity hover:brightness-110 focus-visible:ring-2 focus-visible:ring-wb-accent/50 focus-visible:outline-none disabled:opacity-30"
			aria-label={busy ? 'Queue message' : 'Send'}
			title={busy ? 'Claude will read this after the current step' : 'Send (Enter)'}
			disabled={!canSend}
			onclick={send}
		>
			<ArrowUpIcon class="size-3.5" />
		</button>
	</div>
</div>
