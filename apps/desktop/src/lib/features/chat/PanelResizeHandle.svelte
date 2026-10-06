<script lang="ts">
	import { TASKS_PANEL_WIDTH, type PanelWidth } from './panel-width.svelte';

	let { size, label }: { size: PanelWidth; label: string } = $props();

	function onpointerdown(e: PointerEvent & { currentTarget: HTMLElement }) {
		if (e.button !== 0) return;
		e.preventDefault(); // no text selection while dragging
		e.currentTarget.setPointerCapture(e.pointerId);
		size.beginDrag(e.clientX);
	}
</script>

<!-- A focusable separator is an interactive widget (WAI-ARIA window splitter), which the a11y checks don't know. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
	role="separator"
	aria-orientation="vertical"
	aria-label={label}
	aria-valuemin={TASKS_PANEL_WIDTH.min}
	aria-valuemax={TASKS_PANEL_WIDTH.max}
	aria-valuenow={size.width}
	tabindex="0"
	class="absolute inset-y-0 -left-1 z-10 w-2 cursor-col-resize touch-none after:absolute after:inset-y-0 after:left-1 after:w-px after:transition-colors hover:after:bg-wb-accent focus-visible:outline-none focus-visible:after:bg-wb-accent"
	{onpointerdown}
	onpointermove={(e) => size.dragTo(e.clientX)}
	onpointerup={() => size.endDrag()}
	onpointercancel={() => size.endDrag()}
	onkeydown={(e) => {
		if (size.key(e.key, e.shiftKey)) e.preventDefault();
	}}
></div>
