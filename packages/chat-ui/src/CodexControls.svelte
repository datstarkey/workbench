<script lang="ts">
	import { onDestroy } from 'svelte';
	import type { AgentChat } from './agent-chat.svelte';
	import { CodexControlsStore } from './codex-controls.svelte';
	import { getChatPlatform } from './platform';
	import { safeExternalUrl } from './codex-helpers';
	let { chat, onThread }: { chat: AgentChat; onThread: (id: string, label: string) => void } =
		$props();
	// svelte-ignore state_referenced_locally
	const controls = new CodexControlsStore(chat, onThread);
	const codexState = $derived(chat.meta?.codex);
	const model = $derived(
		chat.meta?.models.find(
			(m) =>
				m.value === (chat.meta?.modelChoice ?? chat.meta?.model) ||
				m.resolvedModel === chat.meta?.model
		)
	);
	onDestroy(() => controls.dispose());
	const urls = $derived.by(() => {
		const found: string[] = [];
		function walk(v: unknown, depth = 0) {
			if (depth > 6) return;
			const url = safeExternalUrl(v);
			if (url) {
				found.push(url);
				return;
			}
			if (v && typeof v === 'object') Object.values(v).forEach((v) => walk(v, depth + 1));
		}
		walk(controls.result);
		return [...new Set(found)];
	});
</script>

{#if chat.agent === 'codex'}
	<details class="rounded-md border border-wb-hair text-xs text-wb-ink">
		<summary class="cursor-pointer px-3 py-2 text-wb-ink-mute"
			>Codex controls{codexState?.goal ? ` · ${codexState.goal.status} goal` : ''}{codexState?.queue
				.length
				? ` · ${codexState.queue.length} queued`
				: ''}</summary
		>
		<div class="flex max-h-96 flex-col gap-3 overflow-auto p-3">
			<nav class="flex flex-wrap gap-2" aria-label="Codex controls">
				{#each ['actions', 'queue', 'threads', 'goal', 'setup', 'remote', 'voice'] as name}
					<button
						class:active={controls.tab === name}
						onclick={() => {
							controls.tab = name;
							controls.result = null;
						}}>{name}</button
					>
				{/each}
			</nav>
			{#if controls.tab === 'actions'}
				<div class="flex flex-wrap items-center gap-2">
					<button
						disabled={controls.busy || chat.meta?.busy}
						onclick={() => controls.run('compact')}>Compact context</button
					>
					<button disabled={controls.busy || chat.meta?.busy} onclick={() => controls.fork()}
						>Fork into a new chat</button
					>
					{#if chat.hasOlderHistory}<button
							disabled={controls.busy}
							onclick={() => chat.loadOlder().catch((e) => (controls.error = String(e)))}
							>Load earlier messages</button
						>{/if}
				</div>
				<label
					>Conversation title <input
						bind:value={controls.title}
						placeholder={chat.meta?.title ?? 'Name this chat'}
					/></label
				>
				<div class="flex gap-2">
					<button
						disabled={controls.busy || !controls.title.trim()}
						onclick={() => controls.run('rename', { name: controls.title })}>Rename</button
					><button
						disabled={controls.busy || chat.meta?.busy}
						onclick={() => controls.run('archive')}>Archive conversation</button
					>
				</div>
				<label
					>Review <select bind:value={controls.reviewType}
						><option value="uncommittedChanges">Uncommitted changes</option><option
							value="baseBranch">Compare with branch</option
						><option value="commit">Commit</option><option value="custom"
							>Custom instructions</option
						></select
					></label
				>
				{#if controls.reviewType !== 'uncommittedChanges'}<input
						bind:value={controls.reviewValue}
						aria-label="Review target"
						placeholder={controls.reviewType === 'baseBranch'
							? 'main'
							: controls.reviewType === 'commit'
								? 'Commit SHA'
								: 'What should Codex review?'}
					/>{/if}
				<button
					disabled={controls.busy || chat.meta?.busy}
					onclick={() =>
						controls.run('review', {
							type: controls.reviewType,
							...(controls.reviewType === 'baseBranch'
								? { branch: controls.reviewValue }
								: controls.reviewType === 'commit'
									? { sha: controls.reviewValue }
									: controls.reviewType === 'custom'
										? { instructions: controls.reviewValue }
										: {})
						})}>Start native review</button
				>
				{#if codexState?.collaborationModes.length}<label
						>Collaboration <select
							value={codexState.collaborationMode ?? ''}
							onchange={(e) => controls.run('collaboration', { mode: e.currentTarget.value })}
							><option value="" disabled>Codex default</option
							>{#each codexState.collaborationModes as mode}{#if mode.mode}<option value={mode.mode}
										>{mode.name}</option
									>{/if}{/each}</select
						></label
					>{/if}
				{#if model?.serviceTiers?.length}<label
						>Speed <select
							value={codexState?.serviceTier ?? 'default'}
							onchange={(e) => controls.run('serviceTier', { tier: e.currentTarget.value })}
							><option value="default">Default</option>{#each model.serviceTiers as tier}<option
									value={tier.id}>{tier.name}</option
								>{/each}</select
						></label
					>{/if}
				{#if codexState?.approvalPolicy}<p class="text-wb-ink-soft">
						Approvals: {typeof codexState.approvalPolicy === 'string'
							? codexState.approvalPolicy
							: JSON.stringify(codexState.approvalPolicy)} · Sandbox: {String(
							codexState.sandbox?.type ?? 'Codex config'
						)}
					</p>{/if}
			{:else if controls.tab === 'queue'}
				<label
					>Messages during a turn <select bind:value={chat.delivery}
						><option value="steer">Steer the current turn</option><option value="queue"
							>Queue for the next turn</option
						></select
					></label
				>
				<button
					disabled={controls.busy}
					onclick={() => controls.run('queuePause', { paused: !codexState?.queuePaused })}
					>{codexState?.queuePaused ? 'Resume queue' : 'Pause queue'}</button
				>
				<p class="text-wb-ink-soft">
					Queued messages start in order when the current turn finishes. Send now steers the running
					turn.
				</p>
				{#each codexState?.queue ?? [] as item, i (item.id)}
					<textarea
						aria-label="Queued message"
						value={controls.editing[item.id] ?? item.text}
						oninput={(e) => (controls.editing[item.id] = e.currentTarget.value)}
					></textarea>
					<div class="flex flex-wrap gap-2">
						<button
							disabled={controls.busy}
							onclick={() =>
								controls.run('queueUpdate', {
									id: item.id,
									text: controls.editing[item.id] ?? item.text
								})}>Save edit</button
						><button
							disabled={controls.busy || i === 0}
							onclick={() => controls.run('queueReorder', { id: item.id, direction: -1 })}
							>Move up</button
						><button
							disabled={controls.busy || i === (codexState?.queue.length ?? 0) - 1}
							onclick={() => controls.run('queueReorder', { id: item.id, direction: 1 })}
							>Move down</button
						><button
							disabled={controls.busy}
							onclick={() => controls.run('queueSend', { id: item.id })}>Send now</button
						><button
							disabled={controls.busy}
							onclick={() => controls.run('queueDelete', { id: item.id })}>Remove</button
						>{#if item.images}<span>{item.images} images</span>{/if}
					</div>
				{:else}<p class="text-wb-ink-soft">No queued messages.</p>{/each}
			{:else if controls.tab === 'threads'}
				<label
					>Find a conversation <input
						bind:value={controls.search}
						placeholder="Search titles"
					/></label
				>
				<label
					><input type="checkbox" bind:checked={controls.archived} /> Archived conversations</label
				>
				<button disabled={controls.busy} onclick={() => controls.list()}>Search this folder</button>
				{#each controls.threads as thread (thread.id)}<div class="flex items-center gap-2">
						<button onclick={() => onThread(thread.id, thread.name ?? thread.preview ?? 'Codex')}
							>{thread.name || thread.preview || 'Untitled conversation'}</button
						>{#if controls.archived}<button
								disabled={controls.busy}
								onclick={async () => {
									await controls.run('unarchive', { threadId: thread.id });
									await controls.list();
								}}>Unarchive</button
							>{/if}
					</div>{/each}
				{#if controls.cursor}<button disabled={controls.busy} onclick={() => controls.list(true)}
						>More conversations</button
					>{/if}
			{:else if controls.tab === 'goal'}
				{#if codexState?.goal}<p>{codexState.goal.objective}</p>
					<p class="text-wb-ink-soft">
						{codexState.goal.status}{codexState.goal.tokensUsed !== undefined
							? ` · ${codexState.goal.tokensUsed.toLocaleString()} tokens used`
							: ''}{codexState.goal.tokenBudget
							? ` · Budget ${codexState.goal.tokenBudget.toLocaleString()} tokens`
							: ''}
					</p>{/if}
				<label
					>Objective <textarea
						bind:value={controls.objective}
						placeholder="What should Codex keep working toward?"
					></textarea></label
				>
				<label
					>Token budget (optional) <input
						type="number"
						min="1"
						step="1"
						value={controls.budget}
						oninput={(e) => (controls.budget = e.currentTarget.value)}
					/></label
				>
				<div class="flex flex-wrap gap-2">
					<button
						disabled={controls.busy || !controls.objective.trim()}
						onclick={() =>
							controls.run('goal', {
								objective: controls.objective,
								...(controls.budget ? { tokenBudget: Number(controls.budget) } : {})
							})}>Set goal</button
					><button
						disabled={controls.busy || !codexState?.goal}
						onclick={() => controls.run('goal', { status: 'paused' })}>Pause</button
					><button
						disabled={controls.busy || !codexState?.goal}
						onclick={() => controls.run('goal', { status: 'active' })}>Resume</button
					><button
						disabled={controls.busy || !codexState?.goal}
						onclick={() => controls.run('clearGoal')}>Clear</button
					>
				</div>
			{:else if controls.tab === 'setup'}
				<label
					>Status <select bind:value={controls.section}
						><option value="account">Account</option><option value="usage">Usage limits</option
						><option value="mcp">MCP servers</option><option value="plugins">Plugins</option><option
							value="hooks">Hooks</option
						><option value="background">Background terminals</option><option value="attachments"
							>Attachments</option
						></select
					></label
				>
				<button
					disabled={controls.busy}
					onclick={() => controls.run('inspect', { section: controls.section })}
					>Refresh status</button
				>
				<button
					disabled={controls.busy}
					onclick={async () => {
						const v = (await controls.run('login')) as { loginId?: string } | null;
						controls.loginId = v?.loginId ?? '';
					}}>Sign in to Codex</button
				>
				{#if controls.loginId}<button
						disabled={controls.busy}
						onclick={() => controls.run('cancelLogin', { loginId: controls.loginId })}
						>Cancel sign in</button
					>{/if}
				<label>MCP server <input bind:value={controls.mcpName} placeholder="Server name" /></label
				><button
					disabled={controls.busy || !controls.mcpName}
					onclick={() => controls.run('mcpLogin', { name: controls.mcpName })}
					>Connect MCP account</button
				>
				{#if controls.section === 'background'}<label
						>Process ID <input bind:value={controls.processId} /></label
					>
					<div class="flex gap-2">
						<button
							disabled={controls.busy || !controls.processId}
							onclick={() => controls.run('backgroundTerminate', { processId: controls.processId })}
							>Stop process</button
						><button disabled={controls.busy} onclick={() => controls.run('backgroundClean')}
							>Stop all background processes</button
						>
					</div>{/if}
			{:else if controls.tab === 'remote'}
				<p class="text-wb-ink-soft">
					Native Codex pairing is experimental and separate from Workbench remote access. It runs
					with this chat process.
				</p>
				<div class="flex flex-wrap gap-2">
					<button
						disabled={controls.busy}
						onclick={() => controls.run('inspect', { section: 'remote' })}>Status</button
					><button disabled={controls.busy} onclick={() => controls.run('remoteEnable')}
						>Enable native access</button
					><button disabled={controls.busy} onclick={() => controls.run('remotePair')}
						>Pair a device</button
					><button disabled={controls.busy} onclick={() => controls.run('remoteDisable')}
						>Disable native access</button
					><button
						disabled={controls.busy}
						onclick={() => controls.run('inspect', { section: 'clients' })}>Paired devices</button
					>
				</div>
				<label>Environment ID <input bind:value={controls.environmentId} /></label><label
					>Device ID <input bind:value={controls.clientId} /></label
				><button
					disabled={controls.busy || !controls.clientId || !controls.environmentId}
					onclick={() =>
						controls.run('remoteRevoke', {
							environmentId: controls.environmentId,
							clientId: controls.clientId
						})}>Revoke device</button
				>
			{:else if controls.tab === 'voice'}
				<p class="text-wb-ink-soft">
					Realtime voice is experimental and requires Codex account support and microphone access.
				</p>
				<button
					disabled={controls.busy}
					onclick={() => controls.run('inspect', { section: 'voices' })}>Available voices</button
				>
				<button
					disabled={controls.busy ||
						(!controls.mic && !codexState?.capabilities.includes('thread/realtime/listVoices'))}
					onclick={() => (controls.mic ? controls.stopVoice() : controls.startVoice())}
					>{controls.mic ? 'Stop microphone' : 'Start voice conversation'}</button
				>
			{/if}
			{#if controls.busy}<p role="status">Working…</p>{/if}
			{#if controls.error}<p class="text-wb-err" role="alert">{controls.error}</p>{/if}
			{#if controls.result && !['queue', 'threads'].includes(controls.tab)}<pre
					class="max-h-40 overflow-auto whitespace-pre-wrap text-wb-ink-mute">{JSON.stringify(
						controls.result,
						null,
						2
					)}</pre>{/if}
			{#each urls as url}<button
					class="break-all text-wb-accent"
					onclick={() => getChatPlatform().openLink(url)}>Open {new URL(url).hostname}</button
				>{/each}
		</div>
	</details>
{/if}

<style>
	button {
		border: 1px solid var(--wb-hair);
		border-radius: 4px;
		padding: 4px 8px;
		text-align: left;
	}
	button:hover:not(:disabled),
	button.active {
		background: var(--wb-panel2);
	}
	button:disabled {
		opacity: 0.45;
	}
	input:not([type='checkbox']),
	select,
	textarea {
		border: 1px solid var(--wb-hair);
		border-radius: 4px;
		padding: 5px 8px;
		background: var(--wb-bg);
		width: 100%;
	}
	label {
		display: flex;
		flex-direction: column;
		gap: 4px;
	}
	label:has(input[type='checkbox']) {
		flex-direction: row;
		align-items: center;
	}
	button:focus-visible,
	input:focus-visible,
	select:focus-visible,
	textarea:focus-visible {
		outline: 2px solid var(--wb-accent);
		outline-offset: 2px;
	}
</style>
