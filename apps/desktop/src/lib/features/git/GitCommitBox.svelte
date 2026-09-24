<script lang="ts">
	import ArchiveIcon from '@lucide/svelte/icons/archive';
	import CheckIcon from '@lucide/svelte/icons/check';
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import LoaderIcon from '@lucide/svelte/icons/loader';
	import { cn } from '@workbench/ui';
	import * as DropdownMenu from '@workbench/ui/dropdown-menu';
	import { primaryButton } from '$features/sidebar/styles';
	import { getGitStore } from '$stores/context';
	import { toast } from 'svelte-sonner';
	import { commitButtonLabel, subjectLength } from './git-view';

	let {
		path,
		stagedCount,
		hasUpstream
	}: { path: string; stagedCount: number; hasUpstream: boolean } = $props();

	const gitStore = getGitStore();
	const SUBJECT_LIMIT = 72;
	const mod = navigator.userAgent.includes('Mac') ? '⌘' : 'Ctrl+';

	let message = $state('');
	let amend = $state(false);
	let busy = $state(false);

	let canCommit = $derived(!busy && message.trim() !== '' && (amend || stagedCount > 0));
	let label = $derived(commitButtonLabel(stagedCount, amend));
	let subject = $derived(subjectLength(message));

	function setAmend(next: boolean) {
		amend = next;
		if (next && !message.trim()) message = gitStore.logByProject[path]?.[0]?.message ?? '';
	}

	async function commit(push: boolean) {
		if (!canCommit) return;
		busy = true;
		const text = message.trim();
		const verb = amend ? 'Amended' : 'Committed';
		try {
			const result = amend
				? await gitStore.commitAmend(path, text)
				: await gitStore.commit(path, text);
			message = '';
			amend = false;
			const sha = result.sha.slice(0, 7);
			if (push) {
				await gitStore.push(path, !hasUpstream);
				toast.success(`${verb} ${sha} and pushed`);
			} else {
				toast.success(`${verb} ${sha}`);
			}
		} catch (e) {
			toast.error(String(e));
		} finally {
			busy = false;
		}
	}

	/** Stashes staged, unstaged and untracked changes in one go */
	async function stash() {
		try {
			await gitStore.stashPush(path);
			toast.success('Changes stashed');
		} catch (e) {
			toast.error(`Failed to stash: ${e}`);
		}
	}

	function onkeydown(e: KeyboardEvent) {
		if (e.key !== 'Enter' || !(e.metaKey || e.ctrlKey)) return;
		e.preventDefault();
		void commit(e.shiftKey && !amend);
	}
</script>

<div class="flex flex-col gap-2 p-3">
	<label for="commit-message" class="sr-only">Commit message</label>
	<textarea
		id="commit-message"
		rows="3"
		placeholder={amend ? 'Amended commit message' : 'Commit message'}
		class="w-full resize-none rounded-md border border-wb-hair bg-wb-bg px-2.5 py-2 text-xs leading-[1.45] text-wb-ink outline-none placeholder:text-wb-ink-soft focus:border-wb-accent focus:ring-[3px] focus:ring-wb-accent-soft"
		bind:value={message}
		{onkeydown}
	></textarea>
	<div class="flex">
		<button
			type="button"
			class={['flex-1 rounded-l-md', primaryButton]}
			disabled={!canCommit}
			onclick={() => commit(false)}
		>
			{#if busy}
				<LoaderIcon class="size-3.5 animate-spin" />
			{:else}
				<CheckIcon class="size-3.5" />
			{/if}
			{label}
		</button>
		<DropdownMenu.Root>
			<DropdownMenu.Trigger
				class={cn(primaryButton, 'w-7 rounded-r-md border-l border-wb-accent-ink/25 px-0')}
				aria-label="More commit options"
				disabled={busy}
			>
				<ChevronDownIcon class="size-3.5" />
			</DropdownMenu.Trigger>
			<DropdownMenu.Content align="end" class="w-52">
				<DropdownMenu.CheckboxItem checked={amend} onCheckedChange={setAmend}>
					Amend last commit
				</DropdownMenu.CheckboxItem>
				<DropdownMenu.Separator />
				<DropdownMenu.Item disabled={!canCommit || amend} onclick={() => commit(true)}>
					Commit & push
				</DropdownMenu.Item>
				<DropdownMenu.Separator />
				<DropdownMenu.Item onclick={stash}>
					<ArchiveIcon class="size-3.5" /> Stash all changes
				</DropdownMenu.Item>
			</DropdownMenu.Content>
		</DropdownMenu.Root>
	</div>
	<div class="flex justify-between text-[10.5px] text-wb-ink-soft">
		<span>{mod}↵ commit · {mod}⇧↵ commit & push</span>
		<span class={['font-mono', subject > SUBJECT_LIMIT && 'text-wb-warn']}>
			{subject}/{SUBJECT_LIMIT}
		</span>
	</div>
</div>
