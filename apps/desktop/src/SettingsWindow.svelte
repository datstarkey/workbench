<script lang="ts">
	import SettingsContent from '$components/settings/SettingsContent.svelte';
	import {
		setClaudeSettingsStore,
		setTrelloStore,
		setWorkbenchSettingsStore
	} from '$stores/context';
	import { ClaudeSettingsStore } from '$stores/claude-settings.svelte';
	import { TrelloStore } from '$stores/trello.svelte';
	import { WorkbenchSettingsStore } from '$stores/workbench-settings.svelte';
	import { getCurrentWindow } from '@tauri-apps/api/window';
	import { onMount } from 'svelte';
	import { watch } from 'runed';

	// Active project is passed from the main window via the query string so
	// Claude project-scope settings and Trello project config resolve correctly.
	const projectPath = new URLSearchParams(window.location.search).get('project');

	// This window runs in its own webview, so it gets its own store instances.
	// All three persist to disk via IPC, so re-instantiation is safe.
	const workbenchSettingsStore = setWorkbenchSettingsStore(new WorkbenchSettingsStore());
	const claudeSettingsStore = setClaudeSettingsStore(new ClaudeSettingsStore());
	const trelloStore = setTrelloStore(new TrelloStore());

	// Apply the selected accent preset to this window's document root too.
	watch(
		() => workbenchSettingsStore.accentColor,
		(accent) => document.documentElement.setAttribute('data-accent', accent)
	);

	function debounce<T extends unknown[]>(fn: (...args: T) => void, ms: number) {
		let timer: ReturnType<typeof setTimeout>;
		return (...args: T) => {
			clearTimeout(timer);
			timer = setTimeout(() => fn(...args), ms);
		};
	}

	onMount(async () => {
		const win = getCurrentWindow();

		// The window is created hidden (visible: false) to avoid a blank white flash
		// while the bundle boots. onMount runs after the shell has painted, so reveal
		// it now — the nav/header render immediately and data loads behind spinners.
		await win.show();
		await win.setFocus();

		await Promise.all([
			workbenchSettingsStore.load(),
			claudeSettingsStore.load(projectPath),
			trelloStore.loadCredentials()
		]);
		if (projectPath) await trelloStore.loadProjectConfig(projectPath);

		// Persist native window geometry (debounced) so it restores next session.
		// Skipped while there are unsaved edits so a resize never flushes a draft.
		const saveBounds = debounce(() => {
			if (workbenchSettingsStore.dirty) return;
			void (async () => {
				const scale = await win.scaleFactor();
				const size = await win.innerSize();
				const pos = await win.outerPosition();
				await workbenchSettingsStore.setSettingsWindowBounds({
					width: Math.round(size.width / scale),
					height: Math.round(size.height / scale),
					x: Math.round(pos.x / scale),
					y: Math.round(pos.y / scale)
				});
			})();
		}, 400);
		void win.onResized(saveBounds);
		void win.onMoved(saveBounds);
	});
</script>

<div class="dark flex h-screen w-screen flex-col overflow-hidden">
	<SettingsContent {projectPath} />
</div>
