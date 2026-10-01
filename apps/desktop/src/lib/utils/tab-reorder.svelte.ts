/**
 * HTML5 drag-to-reorder for a tab strip. Each strip owns an instance, so a
 * drag only drops onto tabs of the strip it started in.
 */
export class TabReorder {
	dragging = $state<string | null>(null);
	over = $state<string | null>(null);

	private ids: () => string[];
	private onMove: (fromId: string, toId: string) => void;

	constructor(ids: () => string[], onMove: (fromId: string, toId: string) => void) {
		this.ids = ids;
		this.onMove = onMove;
	}

	/** Which edge of `id` the dragged tab will land on, for the drop indicator. */
	dropSide(id: string): 'before' | 'after' | null {
		if (!this.dragging || this.over !== id || this.dragging === id) return null;
		const ids = this.ids();
		return ids.indexOf(this.dragging) < ids.indexOf(id) ? 'after' : 'before';
	}

	handlers(id: string) {
		return {
			draggable: true,
			ondragstart: (event: DragEvent) => {
				this.dragging = id;
				if (!event.dataTransfer) return;
				event.dataTransfer.effectAllowed = 'move';
				// A custom type, not text/plain: a stray drop into xterm would type it.
				event.dataTransfer.setData('application/x-workbench-tab', id);
			},
			ondragover: (event: DragEvent) => {
				if (!this.dragging) return;
				event.preventDefault();
				this.over = id;
			},
			ondragleave: () => {
				if (this.over === id) this.over = null;
			},
			ondrop: (event: DragEvent) => {
				event.preventDefault();
				if (this.dragging) this.onMove(this.dragging, id);
				this.reset();
			},
			ondragend: () => this.reset()
		};
	}

	private reset() {
		this.dragging = null;
		this.over = null;
	}
}
