<script lang="ts">
	import ExternalLinkIcon from '@lucide/svelte/icons/external-link';
	import PanelsTopLeftIcon from '@lucide/svelte/icons/panels-top-left';
	import type { ArtifactInfo } from '@workbench/types';
	import { artifactActionLabel, artifactLink, artifactName } from './artifacts';
	import { getChatPlatform } from './platform';

	let { artifact }: { artifact: ArtifactInfo } = $props();

	const platform = getChatPlatform();
	const href = $derived(artifactLink(artifact.url));
</script>

<div class="flex items-center gap-2.5 rounded-md border border-wb-hair bg-wb-panel px-2.5 py-2">
	<span
		class="grid size-7 shrink-0 place-items-center rounded-md bg-wb-panel2 text-[var(--wb-agent,var(--wb-claude))]"
		aria-hidden="true"
	>
		<PanelsTopLeftIcon class="size-3.5" />
	</span>
	<div class="flex min-w-0 flex-1 flex-col">
		<span class="truncate text-xs font-medium text-wb-ink">{artifactName(artifact)}</span>
		<span class="truncate text-[11px] text-wb-ink-soft">
			{artifactActionLabel(artifact.action)} artifact
		</span>
	</div>
	{#if href}
		<button
			type="button"
			class="flex shrink-0 items-center gap-1 rounded-md border border-wb-hair px-2 py-1 text-[11px] text-wb-ink hover:border-wb-ink-soft focus-visible:ring-1 focus-visible:ring-wb-accent focus-visible:outline-none"
			onclick={() => platform.openLink(href)}
		>
			Open <ExternalLinkIcon class="size-3" aria-hidden="true" />
		</button>
	{/if}
</div>
