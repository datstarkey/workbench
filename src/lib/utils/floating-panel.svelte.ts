/** Bounds of a floating panel, in viewport pixels. */
export interface PanelBounds {
	x: number;
	y: number;
	width: number;
	height: number;
}

interface FloatingPanelOptions {
	defaultWidth: number;
	defaultHeight: number;
	minWidth?: number;
	minHeight?: number;
	/** Minimum gap kept between the panel and the viewport edges. */
	margin?: number;
	/** Called with the final bounds when a drag/resize gesture ends. */
	onCommit?: (bounds: PanelBounds) => void;
}

const clamp = (value: number, min: number, max: number) =>
	Math.min(Math.max(value, min), Math.max(min, max));

/**
 * Reactive position + size for a draggable, resizable floating panel.
 *
 * State (`x`/`y`/`width`/`height`) is exposed as `$state` so a component can bind
 * it to inline styles. `startDrag`/`startResize` are pointer-event handlers that
 * track the gesture on `window` and clamp the panel inside the viewport; `onCommit`
 * fires once on release so callers can persist the result.
 */
export class FloatingPanel {
	x = $state(0);
	y = $state(0);
	width = $state(0);
	height = $state(0);

	private readonly minWidth: number;
	private readonly minHeight: number;
	private readonly margin: number;
	private readonly onCommit?: (bounds: PanelBounds) => void;

	constructor(opts: FloatingPanelOptions) {
		this.minWidth = opts.minWidth ?? 320;
		this.minHeight = opts.minHeight ?? 240;
		this.margin = opts.margin ?? 8;
		this.onCommit = opts.onCommit;
		this.width = opts.defaultWidth;
		this.height = opts.defaultHeight;
		this.center();
	}

	get bounds(): PanelBounds {
		return { x: this.x, y: this.y, width: this.width, height: this.height };
	}

	private viewport() {
		return { w: window.innerWidth, h: window.innerHeight };
	}

	/** Center the panel in the current viewport, clamping its size to fit. */
	center() {
		const { w, h } = this.viewport();
		this.width = clamp(this.width, this.minWidth, w - this.margin * 2);
		this.height = clamp(this.height, this.minHeight, h - this.margin * 2);
		this.x = Math.round((w - this.width) / 2);
		this.y = Math.round((h - this.height) / 2);
	}

	/** Apply saved bounds, clamped to the current viewport; centers when null. */
	apply(bounds: PanelBounds | null | undefined) {
		if (!bounds) {
			this.center();
			return;
		}
		const { w, h } = this.viewport();
		this.width = clamp(bounds.width, this.minWidth, w - this.margin * 2);
		this.height = clamp(bounds.height, this.minHeight, h - this.margin * 2);
		this.x = clamp(bounds.x, this.margin, w - this.width - this.margin);
		this.y = clamp(bounds.y, this.margin, h - this.height - this.margin);
	}

	startDrag = (event: PointerEvent) => {
		if (event.button !== 0) return;
		event.preventDefault();
		const startX = event.clientX;
		const startY = event.clientY;
		const originX = this.x;
		const originY = this.y;
		const move = (e: PointerEvent) => {
			const { w, h } = this.viewport();
			this.x = clamp(originX + (e.clientX - startX), this.margin, w - this.width - this.margin);
			this.y = clamp(originY + (e.clientY - startY), this.margin, h - this.height - this.margin);
		};
		this.track(move);
	};

	startResize = (event: PointerEvent) => {
		if (event.button !== 0) return;
		event.preventDefault();
		event.stopPropagation();
		const startX = event.clientX;
		const startY = event.clientY;
		const originW = this.width;
		const originH = this.height;
		const move = (e: PointerEvent) => {
			const { w, h } = this.viewport();
			this.width = clamp(originW + (e.clientX - startX), this.minWidth, w - this.x - this.margin);
			this.height = clamp(originH + (e.clientY - startY), this.minHeight, h - this.y - this.margin);
		};
		this.track(move);
	};

	private track(move: (e: PointerEvent) => void) {
		const up = () => {
			window.removeEventListener('pointermove', move);
			window.removeEventListener('pointerup', up);
			this.onCommit?.(this.bounds);
		};
		window.addEventListener('pointermove', move);
		window.addEventListener('pointerup', up);
	}
}
