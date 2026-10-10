<script lang="ts">
	import { onDestroy, tick, type Snippet } from 'svelte';
	import type { Attachment } from 'svelte/attachments';
	import { watch } from 'runed';
	import ArrowUpIcon from '@lucide/svelte/icons/arrow-up';
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import FileTextIcon from '@lucide/svelte/icons/file-text';
	import MicIcon from '@lucide/svelte/icons/mic';
	import PaperclipIcon from '@lucide/svelte/icons/paperclip';
	import SquareIcon from '@lucide/svelte/icons/square';
	import ZapIcon from '@lucide/svelte/icons/zap';
	import XIcon from '@lucide/svelte/icons/x';
	import { cn } from '@workbench/ui';
	import * as DropdownMenu from '@workbench/ui/dropdown-menu';
	import type {
		AgentKind,
		ChatAttachment,
		ChatFile,
		ChatImage,
		CodexMode,
		PermissionMode,
		SlashCommand
	} from '@workbench/types';
	import {
		agentName,
		insertCommand,
		isRiskyMode,
		isWholeCommand,
		matchCommands,
		modeLabel,
		modeOptions,
		slashQuery
	} from './chat-format';
	import ChatMenu from './ChatMenu.svelte';
	import { addAttachments, fileToAttachment, filesIn, previewUrl } from './attachment-intake';
	import { insertMention, matchFiles, mentionQuery } from './file-mentions';
	import { getChatPlatform } from './platform';
	import { Dictation, insertDictation } from './dictation.svelte';

	let {
		id,
		agent = 'claude',
		draft = $bindable(''),
		images = $bindable<ChatImage[]>([]),
		files = $bindable<ChatFile[]>([]),
		mode,
		busy,
		stoppable,
		sendNow = false,
		disabledReason,
		onSend,
		onStop,
		onMode,
		controls,
		commands = [],
		onCommand,
		loadFiles,
		popover
	}: {
		id: string;
		agent?: AgentKind;
		draft?: string;
		/** Hosts may retain image attachments when navigating away. */
		images?: ChatImage[];
		/** PDF and text attachments for either agent. */
		files?: ChatFile[];
		mode: PermissionMode | CodexMode | null;
		busy: boolean;
		/** Whether Stop shows: also while only background agents run (the chat is idle). */
		stoppable: boolean;
		/** Whether Send now shows (Ctrl/⌘+Enter): ends the running turn instead of joining it. */
		sendNow?: boolean;
		/** Set when nothing can be sent right now; shown as the placeholder. */
		disabledReason: string | null;
		/** Returns false if the message could not be sent (the draft is kept). */
		onSend: (
			text: string,
			images: ChatImage[],
			files: ChatFile[],
			now: boolean
		) => boolean | Promise<boolean>;
		onStop: () => void;
		onMode: (mode: PermissionMode | CodexMode) => void;
		/** More pickers for the toolbar (model, effort). */
		controls?: Snippet;
		/** For the `/` menu. */
		commands?: SlashCommand[];
		/** Commands handled in the app rather than by the agent; true when it took it. */
		onCommand?: (name: string) => boolean;
		/** Paths in the session's cwd for the `@` menu; absent leaves it out. */
		loadFiles?: () => Promise<string[]>;
		/** Shown above the composer, e.g. the resume picker. */
		popover?: Snippet;
	} = $props();

	const name = $derived(agentName(agent));
	const modes = $derived(modeOptions(agent));
	const placeholder = $derived(
		agent === 'codex'
			? 'Message Codex, / for skills, @ for files, or attach a file'
			: 'Message Claude, / for commands, @ for files, or attach a file'
	);

	const platform = getChatPlatform();
	const enterSends = platform.enterSends ?? true;
	const dictation = platform.dictate ? new Dictation(platform.dictate) : null;
	onDestroy(() => dictation?.dispose());

	let attachError = $state('');
	/** A file is being dragged over this composer. */
	let dropping = $state(false);
	let picker: HTMLInputElement | null = null;
	let textarea: HTMLTextAreaElement | null = null;

	/** The `/` and `@` menus: matches for what's being typed, unless dismissed with Esc. */
	let menuIndex = $state(0);
	let dismissedAt = $state<string | null>(null);
	/** The caret, read on input/keyup/click, counts only for the draft it was read on. */
	let caretAt = $state({ draft: '', caret: 0 });
	const caret = $derived(caretAt.draft === draft ? caretAt.caret : draft.length);
	/** Esc dismisses the menu for this text and caret only. */
	const menuKey = $derived(`${caret}:${draft}`);
	const slash = $derived(slashQuery(draft, caret));
	/** A command typed after other text: Enter sends the message, Tab or a click picks. */
	const midLine = $derived(slash !== null && !isWholeCommand(draft, slash, caret));
	const matches = $derived(slash ? matchCommands(commands, slash.query) : []);
	const menuOpen = $derived(matches.length > 0 && dismissedAt !== menuKey && !disabledReason);

	let paths = $state.raw<string[]>([]);
	const mention = $derived(loadFiles && !menuOpen ? mentionQuery(draft, caret) : null);
	const fileMatches = $derived(mention ? matchFiles(paths, mention.query) : []);
	const filesOpen = $derived(fileMatches.length > 0 && dismissedAt !== menuKey && !disabledReason);
	const options = $derived(menuOpen ? matches.length : filesOpen ? fileMatches.length : 0);
	const active = $derived(Math.min(menuIndex, options - 1));

	/** Track the caret and fetch the file list (cached by the chat) once `@` is typed. */
	function onCaret(node: HTMLTextAreaElement) {
		caretAt = { draft: node.value, caret: node.selectionStart };
		if (loadFiles && mentionQuery(node.value, node.selectionStart)) {
			void loadFiles().then((list) => (paths = list));
		}
	}

	async function pickFile(path: string) {
		if (!mention || !textarea) return;
		const next = insertMention(draft, mention, caret, path);
		menuIndex = 0;
		await place(next);
	}

	/** Set the draft and put the caret where an inserted token ends. */
	async function place(next: { text: string; caret: number }) {
		draft = next.text;
		caretAt = { draft: next.text, caret: next.caret };
		await tick();
		textarea?.setSelectionRange(next.caret, next.caret);
	}

	function dictate() {
		if (!dictation || disabledReason !== null) return;
		void dictation.start(async (text) => {
			if (disabledReason !== null) return;
			await place(insertDictation(draft, text, textarea?.selectionStart, textarea?.selectionEnd));
			textarea?.focus();
		});
	}

	/**
	 * A command that is the whole draft can be handled here or sent at once;
	 * one typed mid-line is inserted where it stands, for Claude to expand.
	 */
	async function pick(command: SlashCommand, sendNow: boolean) {
		if (!slash) return;
		menuIndex = 0;
		const whole = !midLine;
		if (whole && onCommand?.(command.name)) {
			draft = '';
			return;
		}
		if (whole && sendNow && !command.argumentHint) {
			draft = `/${command.name}`;
			send();
			return;
		}
		await place(insertCommand(draft, slash, caret, command.name));
	}

	let sending = $state(false);
	const canSend = $derived(
		!sending &&
			!disabledReason &&
			(draft.trim().length > 0 || images.length > 0 || files.length > 0)
	);

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

	function attach(added: ChatAttachment[], error: string | null) {
		const next = addAttachments({ images, files }, added);
		images = next.images;
		files = next.files;
		attachError = error ?? next.error ?? '';
	}

	async function addFiles(picked: File[]) {
		attachError = '';
		const results = await Promise.allSettled(picked.map((f) => fileToAttachment(f)));
		const failed = results.find((r): r is PromiseRejectedResult => r.status === 'rejected');
		attach(
			results.flatMap((r) => (r.status === 'fulfilled' ? [r.value] : [])),
			failed
				? failed.reason instanceof Error
					? failed.reason.message
					: String(failed.reason)
				: null
		);
	}

	function onPaste(event: ClipboardEvent) {
		const pasted = filesIn(event.clipboardData?.items);
		if (pasted.length === 0) return;
		event.preventDefault();
		void addFiles(pasted);
	}

	/** OS file drops the host reports (desktop); a phone pastes or picks instead. */
	const dropTarget: Attachment<HTMLElement> = (node) =>
		platform.watchDrops?.(node, {
			hover: (over) => (dropping = over && !disabledReason),
			drop: (dropped, error) => {
				dropping = false;
				if (!disabledReason) attach(dropped, error);
			}
		});

	async function send(now = false) {
		const typed = /^\/(\S+)$/.exec(draft.trim());
		if (typed && images.length === 0 && files.length === 0 && onCommand?.(typed[1])) {
			draft = '';
			return;
		}
		if (!canSend) return;
		const sentDraft = draft;
		const sentImages = images;
		const sentFiles = files;
		sending = true;
		try {
			if (!(await onSend(sentDraft, sentImages, sentFiles, now && sendNow))) return;
		} finally {
			sending = false;
		}
		// A queued send waits for acknowledgment; preserve edits made while waiting.
		if (draft !== sentDraft || images !== sentImages || files !== sentFiles) return;
		draft = '';
		images = [];
		files = [];
		attachError = '';
	}

	function onKeydown(event: KeyboardEvent) {
		if (options > 0) {
			const step = event.key === 'ArrowDown' ? 1 : event.key === 'ArrowUp' ? -1 : 0;
			if (step) {
				event.preventDefault();
				menuIndex = (active + step + options) % options;
				return;
			}
			const enter = event.key === 'Enter' && !event.shiftKey && !event.isComposing;
			if ((enter && !(menuOpen && midLine)) || event.key === 'Tab') {
				event.preventDefault();
				if (menuOpen) void pick(matches[active], event.key === 'Enter' && enterSends);
				else void pickFile(fileMatches[active]);
				return;
			}
			if (event.key === 'Escape') {
				event.preventDefault();
				event.stopPropagation(); // Esc here closes the menu, not the turn
				dismissedAt = menuKey;
				return;
			}
		}
		if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) {
			const now = sendNow && (event.ctrlKey || event.metaKey);
			if (!enterSends && !now) return;
			event.preventDefault();
			send(now);
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
		<ChatMenu
			id="{id}-menu"
			label="Commands"
			items={matches}
			key={(command) => command.name}
			{active}
			onPick={(command) => void pick(command, true)}
			onHover={(i) => (menuIndex = i)}
		>
			{#snippet row(command)}
				<span class="shrink-0 font-mono text-wb-ink">/{command.name}</span>
				{#if command.argumentHint}
					<span class="shrink-0 font-mono text-[11px] text-wb-ink-soft">{command.argumentHint}</span
					>
				{/if}
				<span class="min-w-0 truncate text-wb-ink-mute">{command.description}</span>
			{/snippet}
		</ChatMenu>
	{:else if filesOpen}
		<ChatMenu
			id="{id}-menu"
			label="Files"
			items={fileMatches}
			key={(path) => path}
			{active}
			onPick={(path) => void pickFile(path)}
			onHover={(i) => (menuIndex = i)}
		>
			{#snippet row(path)}
				{@const cut = path.lastIndexOf('/')}
				<span class="shrink-0 font-mono text-wb-ink">{path.slice(cut + 1)}</span>
				<span class="min-w-0 truncate font-mono text-[11px] text-wb-ink-soft">
					{path.slice(0, cut + 1)}
				</span>
			{/snippet}
		</ChatMenu>
	{/if}
	{#if dropping}
		<div
			class="pointer-events-none absolute inset-0 z-10 flex items-center justify-center rounded-xl bg-wb-accent-soft text-xs font-medium text-wb-ink"
		>
			Drop files to attach
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
	{#if files.length > 0}
		<ul
			class={cn('flex flex-wrap gap-1.5 px-3', images.length > 0 ? 'pt-2' : 'pt-3')}
			aria-label="Attached files"
		>
			{#each files as file, i (i)}
				<li
					class="thumb flex max-w-56 items-center gap-1.5 rounded-md border border-wb-hair bg-wb-panel2 py-1 pr-1 pl-2 text-xs text-wb-ink"
				>
					<FileTextIcon class="size-3.5 shrink-0 text-wb-ink-mute" aria-hidden="true" />
					<span class="min-w-0 truncate">{file.name}</span>
					<button
						type="button"
						class="flex size-5 shrink-0 items-center justify-center rounded text-wb-ink-mute hover:bg-wb-panel hover:text-wb-ink focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none"
						aria-label="Remove {file.name}"
						onclick={() => (files = files.filter((_, j) => j !== i))}
					>
						<XIcon class="size-3" />
					</button>
				</li>
			{/each}
		</ul>
	{/if}
	<label for={id} class="sr-only">Message {name}</label>
	<textarea
		{id}
		bind:value={draft}
		{@attach (node: HTMLTextAreaElement) => {
			textarea = node;
		}}
		{@attach autosize}
		onkeydown={onKeydown}
		onpaste={onPaste}
		oninput={(e) => {
			menuIndex = 0;
			onCaret(e.currentTarget);
		}}
		onkeyup={(e) => onCaret(e.currentTarget)}
		onclick={(e) => onCaret(e.currentTarget)}
		role="combobox"
		aria-expanded={options > 0}
		aria-controls={options > 0 ? `${id}-menu` : undefined}
		aria-autocomplete="list"
		aria-activedescendant={options > 0 ? `${id}-menu-${active}` : undefined}
		rows="1"
		enterkeyhint={enterSends ? 'send' : 'enter'}
		disabled={disabledReason !== null}
		placeholder={disabledReason ?? placeholder}
		class="scrollbar-thin block max-h-[180px] w-full resize-none bg-transparent px-3.5 pt-3 pb-1 text-sm leading-relaxed text-wb-ink placeholder:text-wb-ink-soft focus:outline-none disabled:cursor-not-allowed"
	></textarea>
	{#if attachError}
		<p class="px-3.5 pb-1 text-[11px] text-wb-err" role="alert">{attachError}</p>
	{/if}
	{#if dictation?.error}
		<p class="px-3.5 pb-1 text-[11px] text-wb-err" role="alert">{dictation.error}</p>
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
							'flex min-w-0 items-center gap-1 rounded-md px-2 py-1 text-xs whitespace-nowrap hover:bg-wb-panel2 focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none disabled:opacity-50',
							isRiskyMode(mode) ? 'text-wb-err' : 'text-wb-ink-mute'
						)}
						title={agent === 'codex' ? 'Approvals and sandbox' : 'Permission mode'}
					>
						<span class="truncate">{modeLabel(mode, agent)}</span>
						<ChevronDownIcon class="size-3 shrink-0" />
					</button>
				{/snippet}
			</DropdownMenu.Trigger>
			<DropdownMenu.Content align="start" class="w-64">
				<DropdownMenu.RadioGroup
					value={mode ?? (agent === 'codex' ? '' : 'default')}
					onValueChange={(value) => onMode(value as PermissionMode | CodexMode)}
				>
					{#each modes as option (option.mode)}
						<DropdownMenu.RadioItem value={option.mode} class="flex-col items-start gap-0">
							<span class={cn(option.risky && 'text-wb-err')}>
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
			class="flex size-7 shrink-0 items-center justify-center rounded-md text-wb-ink-mute hover:bg-wb-panel2 hover:text-wb-ink focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none disabled:opacity-50"
			title="Attach images, PDFs or text files"
			aria-label="Attach files"
			disabled={disabledReason !== null}
			onclick={() => picker?.click()}
		>
			<PaperclipIcon class="size-3.5" />
		</button>
		<input
			{@attach (node: HTMLInputElement) => {
				picker = node;
			}}
			type="file"
			multiple
			class="hidden"
			onchange={(e) => {
				void addFiles(filesIn(e.currentTarget.files));
				e.currentTarget.value = '';
			}}
		/>
		<span class="flex-1"></span>
		{#if dictation}
			<button
				type="button"
				class="flex size-9 shrink-0 items-center justify-center rounded-md text-wb-ink-mute hover:bg-wb-panel2 hover:text-wb-ink focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none disabled:opacity-50"
				aria-label={dictation.busy ? 'Listening…' : 'Dictate message'}
				title={dictation.busy ? 'Listening…' : 'Dictate message'}
				disabled={disabledReason !== null || dictation.busy}
				onclick={dictate}
			>
				<MicIcon class={cn('size-4', dictation.busy && 'text-wb-accent')} />
			</button>
		{/if}
		{#if stoppable}
			<button
				type="button"
				class="flex size-7 shrink-0 items-center justify-center rounded-lg border border-wb-hair bg-wb-panel2 text-wb-ink-mute hover:text-wb-ink focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none"
				title="Stop (Esc)"
				aria-label="Stop"
				onclick={onStop}
			>
				<SquareIcon class="size-2.5 fill-current" />
			</button>
		{/if}
		{#if sendNow && busy}
			<button
				type="button"
				class="flex size-7 shrink-0 items-center justify-center rounded-lg border border-wb-hair bg-wb-panel2 text-wb-ink-mute hover:text-wb-ink focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none disabled:opacity-30"
				aria-label="Send now"
				title={`Send now${enterSends ? ' (Ctrl+Enter)' : ''}: stops this turn, running commands move to the background`}
				disabled={!canSend}
				onclick={() => send(true)}
			>
				<ZapIcon class="size-3.5" />
			</button>
		{/if}
		<button
			type="button"
			class="flex size-7 shrink-0 items-center justify-center rounded-lg bg-wb-accent text-wb-accent-ink transition-opacity hover:brightness-110 focus-visible:ring-2 focus-visible:ring-wb-accent/50 focus-visible:outline-none disabled:opacity-30"
			aria-label={busy ? 'Queue message' : 'Send'}
			title={busy
				? `${name} will read this after the current step`
				: enterSends
					? 'Send (Enter)'
					: 'Send'}
			disabled={!canSend}
			onclick={() => send()}
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
