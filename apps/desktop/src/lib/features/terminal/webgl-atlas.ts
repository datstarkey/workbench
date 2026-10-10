import { WebglAddon } from '@xterm/addon-webgl';
import type { Terminal } from '@xterm/xterm';

/**
 * A terminal's WebGL renderer, with xterm's glyph atlas kept to a budget.
 *
 * The atlas is shared by every terminal with the same font and theme and never
 * evicts: each new glyph + colour pair takes space, and once 16 pages of 512² exist
 * it merges four into one twice the size, up to the GPU's max texture size (16384²,
 * 1 GB). `clearTextureAtlas()` doesn't help: merged pages stay allocated, and the
 * page count still creeps up to the next merge. Claude Code's truecolor spinner and
 * shimmer mint new colours whenever it works, so a day of sessions took the webview
 * past 2 GB until WebKit killed its GPU process and left the window black.
 *
 * The atlas is only freed once every terminal releases it, so past the budget every
 * renderer is dropped before any reloads, and the reloads build a fresh atlas.
 * Reloading one at a time would just take the old atlas back from xterm's cache.
 */

/** xterm's atlas page size; the only other size below the budget is its fixed
 * overflow page (max texture size) for glyphs wider than a page, which never grows. */
const PAGE_SIZE = 512;

/** Below the 16 pages at which xterm starts merging. */
export const ATLAS_PAGE_BUDGET = 12;

const renderers = new Set<WebglRenderer>();
let reloadQueued = false;

function atlasPages(): number {
	const pages = new Set<HTMLCanvasElement>();
	for (const r of renderers) for (const p of r.pages) if (p.width === PAGE_SIZE) pages.add(p);
	return pages.size;
}

function checkBudget(): void {
	if (reloadQueued || atlasPages() <= ATLAS_PAGE_BUDGET) return;
	reloadQueued = true;
	// Pages are added mid-render; swap renderers once the frame is done.
	setTimeout(reloadAll, 0);
}

function reloadAll(): void {
	reloadQueued = false;
	const all = [...renderers];
	for (const r of all) r.drop();
	for (const r of all) r.load();
}

export class WebglRenderer {
	/** Pages of this renderer's current atlas that it has seen. */
	pages = new Set<HTMLCanvasElement>();
	private addon: WebglAddon | null = null;

	constructor(
		private readonly terminal: Terminal,
		private readonly onContextLoss: () => void
	) {}

	/** Without WebGL the terminal keeps its DOM renderer. */
	load(): void {
		if (this.addon) return;
		try {
			const addon = new WebglAddon();
			this.addon = addon;
			renderers.add(this);
			// Fires on every atlas acquire (first page included); a new atlas, e.g. after
			// a DPR change, sends no remove events for the old one's pages.
			addon.onChangeTextureAtlas((page) => (this.pages = new Set([page])));
			addon.onAddTextureAtlasCanvas((page) => {
				this.pages.add(page);
				checkBudget();
			});
			addon.onRemoveTextureAtlasCanvas((page) => this.pages.delete(page));
			addon.onContextLoss(() => {
				this.drop();
				this.onContextLoss();
			});
			this.terminal.loadAddon(addon);
		} catch {
			this.drop();
		}
	}

	drop(): void {
		renderers.delete(this);
		this.pages = new Set();
		const addon = this.addon;
		this.addon = null;
		try {
			addon?.dispose();
		} catch (e) {
			console.warn('[webgl] dispose failed', e);
		}
	}
}
