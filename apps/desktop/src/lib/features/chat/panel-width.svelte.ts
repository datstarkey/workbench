type WidthStorage = Pick<Storage, 'getItem' | 'setItem'>;

export const TASKS_PANEL_WIDTH = { min: 240, max: 640, initial: 288, step: 16 };
const STORAGE_KEY = 'workbench.chat.tasksPanelWidth';

function browserStorage(): WidthStorage | null {
	try {
		return globalThis.localStorage ?? null;
	} catch {
		return null;
	}
}

/**
 * The docked tasks panel's width, set by dragging or arrow-keying its left
 * edge and remembered per viewer. Its edge is on the left, so moving left widens it.
 */
export class PanelWidth {
	width = $state(TASKS_PANEL_WIDTH.initial);
	private drag: { x: number; width: number } | null = null;
	private readonly storage: WidthStorage | null;

	constructor(storage: WidthStorage | null = browserStorage()) {
		this.storage = storage;
		try {
			const saved = Number(storage?.getItem(STORAGE_KEY));
			if (saved) this.width = clamp(saved);
		} catch {
			// Unreadable storage keeps the default.
		}
	}

	beginDrag(x: number): void {
		this.drag = { x, width: this.width };
	}

	dragTo(x: number): void {
		if (this.drag) this.width = clamp(this.drag.width + this.drag.x - x);
	}

	endDrag(): void {
		if (!this.drag) return;
		this.drag = null;
		this.save();
	}

	/** Applies a separator key; false when the key isn't one. */
	key(key: string, shift = false): boolean {
		const next = keyedWidth(this.width, key, shift);
		if (next === null) return false;
		this.width = clamp(next);
		this.save();
		return true;
	}

	private save(): void {
		try {
			this.storage?.setItem(STORAGE_KEY, String(this.width));
		} catch {
			// A full or blocked storage only loses the preference.
		}
	}
}

function keyedWidth(width: number, key: string, shift: boolean): number | null {
	const { min, max, step } = TASKS_PANEL_WIDTH;
	const by = shift ? step * 4 : step;
	switch (key) {
		case 'ArrowLeft':
			return width + by;
		case 'ArrowRight':
			return width - by;
		case 'Home':
			return min;
		case 'End':
			return max;
		default:
			return null;
	}
}

function clamp(width: number): number {
	const { min, max } = TASKS_PANEL_WIDTH;
	return Math.round(Math.min(max, Math.max(min, width)));
}

/** One width for every chat pane, so they stay in step. */
export const tasksPanelWidth = new PanelWidth();
