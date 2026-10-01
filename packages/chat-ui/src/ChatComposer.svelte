<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { Attachment } from 'svelte/attachments';
	import { watch } from 'runed';
	import ArrowUpIcon from '@lucide/svelte/icons/arrow-up';
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import ImagePlusIcon from '@lucide/svelte/icons/image-plus';
	import SquareIcon from '@lucide/svelte/icons/square';
	import XIcon from '@lucide/svelte/icons/x';
	import { cn } from '@workbench/ui';
	import * as DropdownMenu from '@workbench/ui/dropdown-menu';
	import type { ChatImage, PermissionMode, SlashCommand } from '@workbench/types';
	import { matchCommands, MODE_OPTIONS, modeLabel, slashQuery } from './chat-format';
	import ChatSlashMenu from './ChatSlashMenu.svelte';
	import { fileToChatImage, IMAGE_TYPES, imageFiles, MAX_IMAGES, previewUrl } from './image-intake';
	import { getChatPlatform } from './platform';

	let {
		id,
		draft = $bindable(''),
		mode,
		busy,
		disabledReason,
		onSend,
		onStop,
		onMode,
		controls,
		commands = [],
		onCommand,
		popover
	}: {
		id: string;
		draft?: string;
		mode: PermissionMode | null;
		busy: boolean;
		/** Set when nothing can be sent right now; shown as the placeholder. */
		disabledReason: string | null;
		/** Returns false if the message could not be sent (the draft is kept). */
		onSend: (text: string, images: ChatImage[]) => boolean;
		onStop: () => void;
		onMode: (mode: PermissionMode) => void;
		/** More pickers for the toolbar (model, effort). */
		controls?: Snippet;
		/** For the `/` menu. */
		commands?: SlashCommand[];
		/** Commands handled in the app rather than by Claude; true when it took it. */
		onCommand?: (name: string) => boolean;
		/** Shown above the composer, e.g. the resume picker. */
		popover?: Snippet;
	} = $props();

	const platform = getChatPlatform();

	let images = $state<ChatImage[]>([]);
	let imageError = $state('');
	/** A file is being dragged over this composer. */
	let dropping = $state(false);
	let picker: HTMLInputElement | null = null;

	/** The `/` menu: matches for the command being typed, unless dismissed with Esc. */
	let menuIndex = $state(0);
	let dismissedAt = $state<string | null>(null);
	const query = $derived(slashQuery(draft));
	const matches = $derived(query === null ? [] : matchCommands(commands, query));
	const menuOpen = $derived(matches.length > 0 && dismissedAt !== draft && !disabledReason);

	function pick(command: SlashCommand, sendNow: boolean) {
		menuIndex = 0;
		if (onCommand?.(command.name)) {
			draft = '';
			return;
		}
		if (command.argumentHint || !sendNow) {
			draft = `/${command.name} `;
			return;
		}
		draft = `/${command.name}`;
		send();
	}

	const canSend = $derived(!disabledReason && (draft.trim().length > 0 || images.length > 0));

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

	function addImages(added: ChatImage[]) {
		const room = MAX_IMAGES - images.length;
		if (added.length > room) imageError = `Attach up to ${MAX_IMAGES} images per message.`;
		images = [...images, ...added.slice(0, Math.max(0, room))];
	}

	async function addFiles(files: File[]) {
		imageError = '';
		const results = await Promise.allSettled(files.map(fileToChatImage));
		addImages(results.flatMap((r) => (r.status === 'fulfilled' ? [r.value] : [])));
		const failed = results.find((r): r is PromiseRejectedResult => r.status === 'rejected');
		if (failed)
			imageError = failed.reason instanceof Error ? failed.reason.message : String(failed.reason);
	}

	function onPaste(event: ClipboardEvent) {
		const files = imageFiles(event.clipboardData?.items);
		if (files.length === 0) return;
		event.preventDefault();
		void addFiles(files);
	}

	/** OS file drops the host reports (desktop); a phone pastes or picks instead. */
	const dropTarget: Attachment<HTMLElement> = (node) =>
		platform.watchImageDrops?.(node, {
			hover: (over) => (dropping = over && !disabledReason),
			drop: (dropped, error) => {
				dropping = false;
				if (disabledReason) return;
				imageError = error ?? '';
				addImages(dropped);
			}
		});

	function send() {
		const typed = /^\/(\S+)$/.exec(draft.trim());
		if (typed && images.length === 0 && onCommand?.(typed[1])) {
			draft = '';
			return;
		}
		if (!canSend || !onSend(draft, images)) return;
		draft = '';
		images = [];
		imageError = '';
	}

	function onKeydown(event: KeyboardEvent) {
		if (menuOpen) {
			const step = event.key === 'ArrowDown' ? 1 : event.key === 'ArrowUp' ? -1 : 0;
			if (step) {
				event.preventDefault();
				menuIndex =
					(Math.min(menuIndex, matches.length - 1) + step + matches.length) % matches.length;
				return;
			}
			const chosen = matches[Math.min(menuIndex, matches.length - 1)];
			if ((event.key === 'Enter' && !event.shiftKey) || event.key === 'Tab') {
				event.preventDefault();
				pick(chosen, event.key === 'Enter');
				return;
			}
			if (event.key === 'Escape') {
				event.preventDefault();
				event.stopPropagation(); // Esc here closes the menu, not the turn
				dismissedAt = draft;
				return;
			}
		}
		if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) {
			event.preventDefault();
			send();
		}
	}
</script>

<div
	{@attach dropTarget}
	class={cn(
		'composer relative rounded-xl border bg-wb-panel transition-colors focus-within:border-wb-ink-soft',
		dropping ? 'border-wb-accent' : 'border-wb-hair'
	)}
>
	{@render popover?.()}
	{#if menuOpen}
		<ChatSlashMenu
			id="{id}-commands"
			commands={matches}
			active={Math.min(menuIndex, matches.length - 1)}
			onPick={(command) => pick(command, true)}
			onHover={(i) => (menuIndex = i)}
		/>
	{/if}
	{#if dropping}
		<div
			class="pointer-events-none absolute inset-0 z-10 flex items-center justify-center rounded-xl bg-wb-accent-soft text-xs font-medium text-wb-ink"
		>
			Drop images to attach
		</div>
	{/if}
	{#if images.length > 0}
		<ul class="flex flex-wrap gap-2 px-3 pt-3" aria-label="Attached images">
			{#each images as image, i (i)}
				<li class="thumb group relative size-16 overflow-hidden rounded-md border border-wb-hair">
					<img src={previewUrl(image)} alt={image.name} class="size-full object-cover" />
					<button
						type="button"
						class="absolute top-0.5 right-0.5 flex size-5 items-center justify-center rounded-full bg-wb-bg/85 text-wb-ink opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100 focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none"
						aria-label="Remove {image.name}"
						onclick={() => (images = images.filter((_, j) => j !== i))}
					>
						<XIcon class="size-3" />
					</button>
				</li>
			{/each}
		</ul>
	{/if}
	<label for={id} class="sr-only">Message Claude</label>
	<textarea
		{id}
		bind:value={draft}
		{@attach autosize}
		onkeydown={onKeydown}
		onpaste={onPaste}
		oninput={() => (menuIndex = 0)}
		role="combobox"
		aria-expanded={menuOpen}
		aria-controls={menuOpen ? `${id}-commands` : undefined}
		aria-autocomplete="list"
		aria-activedescendant={menuOpen
			? `${id}-commands-${Math.min(menuIndex, matches.length - 1)}`
			: undefined}
		rows="1"
		disabled={disabledReason !== null}
		placeholder={disabledReason ?? 'Message Claude, / for commands, or paste an image'}
		class="scrollbar-thin block max-h-[180px] w-full resize-none bg-transparent px-3.5 pt-3 pb-1 text-sm leading-relaxed text-wb-ink placeholder:text-wb-ink-soft focus:outline-none disabled:cursor-not-allowed"
	></textarea>
	{#if imageError}
		<p class="px-3.5 pb-1 text-[11px] text-wb-err" role="alert">{imageError}</p>
	{/if}
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
		{@render controls?.()}
		<button
			type="button"
			class="flex size-7 items-center justify-center rounded-md text-wb-ink-mute hover:bg-wb-panel2 hover:text-wb-ink focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none disabled:opacity-50"
			title="Attach images"
			aria-label="Attach images"
			disabled={disabledReason !== null}
			onclick={() => picker?.click()}
		>
			<ImagePlusIcon class="size-3.5" />
		</button>
		<input
			{@attach (node: HTMLInputElement) => {
				picker = node;
			}}
			type="file"
			accept={IMAGE_TYPES.join(',')}
			multiple
			class="hidden"
			onchange={(e) => {
				void addFiles(imageFiles(e.currentTarget.files));
				e.currentTarget.value = '';
			}}
		/>
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

<style>
	.thumb {
		animation: pop 180ms cubic-bezier(0.2, 0.8, 0.2, 1);
	}
	@keyframes pop {
		from {
			opacity: 0;
			transform: scale(0.92);
		}
	}
	@media (prefers-reduced-motion: reduce) {
		.thumb {
			animation: none;
		}
	}
</style>
