<script lang="ts">
	import ExternalLinkIcon from '@lucide/svelte/icons/external-link';
	import XIcon from '@lucide/svelte/icons/x';
	import { cn } from '@workbench/ui';
	import { artifactActionLabel, artifactName, type ChatArtifact } from './artifacts';
	import { getChatPlatform } from './platform';

	let {
		artifacts,
		onClose,
		class: className
	}: {
		artifacts: ChatArtifact[];
		onClose?: () => void;
		class?: string;
	} = $props();

	const platform = getChatPlatform();

	function summary(artifact: ChatArtifact): string {
		const label = artifactActionLabel(artifact.action);
		return artifact.publishes > 1 ? `${label} · ${artifact.publishes} versions` : label;
	}
</script>

<aside
	class={cn('flex h-full min-h-0 w-72 flex-col border-l border-wb-hair bg-wb-panel', className)}
	aria-label="Artifacts"
>
	<header class="flex h-9 shrink-0 items-center gap-2 border-b border-wb-hair px-3 text-xs">
		<span class="font-medium text-wb-ink">Artifacts</span>
		<span class="text-wb-ink-soft tabular-nums">{artifacts.length}</span>
		{#if onClose}
			<button
				type="button"
				class="ml-auto rounded p-0.5 text-wb-ink-soft hover:text-wb-ink focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none"
				aria-label="Close panel"
				onclick={onClose}
			>
				<XIcon class="size-3.5" />
			</button>
		{/if}
	</header>
	{#if artifacts.length === 0}
		<p class="px-3.5 pt-3 text-xs leading-relaxed text-wb-ink-soft">
			Artifacts Claude publishes to claude.ai in this chat appear here.
		</p>
	{:else}
		<ul class="scrollbar-thin flex min-h-0 flex-col gap-0.5 overflow-y-auto p-1.5">
			{#each artifacts as artifact (artifact.url)}
				<li>
					<button
						type="button"
						class="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-xs hover:bg-wb-panel2 focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none"
						title={artifact.url}
						onclick={() => platform.openLink(artifact.url)}
					>
						<span class="flex min-w-0 flex-1 flex-col">
							<span class="truncate text-wb-ink">{artifactName(artifact)}</span>
							<span class="truncate text-[11px] text-wb-ink-soft">{summary(artifact)}</span>
						</span>
						<ExternalLinkIcon class="size-3 shrink-0 text-wb-ink-soft" aria-label="Open" />
					</button>
				</li>
			{/each}
		</ul>
	{/if}
</aside>
