<script lang="ts">
	import { onMount } from 'svelte';
	import type { AgentKind, DiscoveredClaudeSession, GitFileStatus } from '@workbench/types';
	import { cn } from '@workbench/ui';
	import type { MobileClient } from './client.svelte';
	import type { ReviewFolder } from './project-review.svelte';
	import Sheet from './Sheet.svelte';

	let {
		client,
		folder,
		initialTab = 'history',
		initialAgent = 'claude',
		accountId,
		onClose
	}: {
		client: MobileClient;
		folder: ReviewFolder;
		initialTab?: 'history' | 'changes';
		initialAgent?: AgentKind;
		accountId?: string;
		onClose: () => void;
	} = $props();
	// svelte-ignore state_referenced_locally
	const review = client.review(folder);
	// svelte-ignore state_referenced_locally
	let tab = $state(initialTab);
	// svelte-ignore state_referenced_locally
	let agent = $state<AgentKind>(initialAgent);
	// svelte-ignore state_referenced_locally
	let account = $state(accountId ?? '');
	let query = $state('');
	let selected = $state<{ file: GitFileStatus; staged: boolean } | null>(null);
	const sessions = $derived(
		review.sessions.filter((s) => s.label.toLowerCase().includes(query.trim().toLowerCase()))
	);

	function refresh() {
		selected = null;
		void (tab === 'history' ? review.history(agent, account || undefined) : review.changes());
	}
	function pick(session: DiscoveredClaudeSession) {
		onClose();
		client.openChat({
			...folder,
			sessionId: session.sessionId,
			agent,
			name: session.label,
			...(agent === 'claude' && session.accountId ? { claudeAccountId: session.accountId } : {})
		});
	}
	function preview(file: GitFileStatus, staged: boolean) {
		selected = { file, staged };
		void review.preview(file, staged);
	}
	onMount(refresh);
</script>

<Sheet
	label="{folder.name} — {tab === 'history' ? 'Conversation history' : 'Review changes'}"
	{onClose}
>
	<div class="flex flex-col gap-3 text-sm">
		<div class="flex items-center gap-2">
			<h2 class="min-w-0 flex-1 truncate font-semibold">{folder.name}</h2>
			<button
				type="button"
				class="h-9 rounded-lg border border-wb-hair px-3 text-xs"
				onclick={onClose}>Close</button
			>
		</div>
		<div class="grid grid-cols-2 gap-1 rounded-lg bg-wb-panel2 p-1">
			{#each ['history', 'changes'] as const as option (option)}
				<button
					type="button"
					aria-pressed={tab === option}
					class={cn('h-9 rounded-md text-xs', tab === option && 'bg-wb-bg text-wb-ink')}
					onclick={() => {
						tab = option;
						refresh();
					}}
				>
					{option === 'history' ? 'Conversation history' : 'Git changes'}
				</button>
			{/each}
		</div>
		<div class="flex flex-wrap items-center gap-2">
			{#if tab === 'history'}
				<label class="flex items-center gap-2 text-xs"
					>Agent
					<select
						class="h-9 rounded-lg border border-wb-hair bg-wb-panel2 px-2"
						bind:value={agent}
						onchange={refresh}
						><option value="claude">Claude</option><option value="codex">Codex</option></select
					>
				</label>
				{#if agent === 'claude' && client.accounts.length}
					<label class="flex items-center gap-2 text-xs"
						>Account
						<select
							class="h-9 max-w-40 rounded-lg border border-wb-hair bg-wb-panel2 px-2"
							bind:value={account}
							onchange={refresh}
						>
							<option value="">Default</option>{#each client.accounts as a (a.id)}<option
									value={a.id}>{a.name}</option
								>{/each}
						</select>
					</label>
				{/if}
			{:else if review.status}
				<span class="min-w-0 flex-1 truncate font-mono text-xs text-wb-ink-mute"
					>{review.status.branch} · {review.status.files.length} changed files</span
				>
			{/if}
			<button
				type="button"
				class="ml-auto h-9 rounded-lg border border-wb-hair px-3 text-xs disabled:opacity-50"
				disabled={review.loading}
				onclick={refresh}>Refresh</button
			>
		</div>
		{#if review.error}<p class="text-xs break-words text-wb-err" role="alert">
				{review.error}
			</p>{/if}
		{#if review.loading}<p class="text-xs text-wb-ink-soft" role="status">Loading…</p>{/if}
		{#if tab === 'history'}
			<input
				aria-label="Search conversations"
				placeholder="Search conversations"
				bind:value={query}
				class="h-10 rounded-lg border border-wb-hair bg-wb-bg px-3 text-sm"
			/>
			{#if !review.loading && !review.error && !sessions.length}<p
					class="py-3 text-xs text-wb-ink-soft"
				>
					{query ? 'No matching conversations.' : 'No saved conversations in this folder.'}
				</p>{/if}
			{#each sessions as session (session.sessionId)}
				<button
					type="button"
					class="flex flex-col gap-1 rounded-lg border border-wb-hair px-3 py-2.5 text-left active:bg-wb-panel2"
					onclick={() => pick(session)}
				>
					<span class="line-clamp-2">{session.label}</span>
					<span class="text-[11px] text-wb-ink-soft"
						>{new Date(session.timestamp).toLocaleString()}</span
					>
				</button>
			{/each}
		{:else}
			{#if selected}
				<button
					type="button"
					class="self-start py-2 text-xs text-wb-accent"
					onclick={() => {
						selected = null;
						void review.changes();
					}}>← Changed files</button
				>
				<p class="font-mono text-xs break-all">
					{selected.file.path} · {selected.staged ? 'staged' : 'working tree'}
				</p>
				{#if review.diff !== null}
					<pre
						class="max-h-[50vh] overflow-auto rounded-lg bg-wb-bg p-3 font-mono text-[11px] leading-relaxed">{#each review.diff.split('\n') as line}<span
								class={cn(
									line.startsWith('+') && 'text-wb-ok',
									line.startsWith('-') && 'text-wb-err'
								)}>{line}{'\n'}</span
							>{/each}</pre>
				{/if}
			{:else}
				{#if review.status && !review.status.files.length}<p class="py-3 text-xs text-wb-ink-soft">
						Working tree is clean.
					</p>{/if}
				{#each review.status?.files ?? [] as file (file.path)}
					<div class="flex flex-col gap-2 rounded-lg border border-wb-hair p-3">
						<span class="font-mono text-xs break-all"
							><span class="text-wb-ink-soft">{file.status}</span> {file.path}</span
						>
						<div class="flex gap-2">
							{#if file.staged}<button
									type="button"
									class="h-9 rounded-md border border-wb-hair px-3 text-xs"
									onclick={() => preview(file, true)}>Staged diff</button
								>{/if}
							{#if file.unstaged || !file.staged}<button
									type="button"
									class="h-9 rounded-md border border-wb-hair px-3 text-xs"
									onclick={() => preview(file, false)}>Working tree diff</button
								>{/if}
						</div>
					</div>
				{/each}
			{/if}
		{/if}
	</div>
</Sheet>
