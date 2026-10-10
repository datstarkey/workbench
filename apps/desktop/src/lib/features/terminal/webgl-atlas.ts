import type { WebglAddon } from '@xterm/addon-webgl';

/**
 * Caps xterm's WebGL glyph atlas, which otherwise grows for as long as the app runs.
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
 * pane drops its WebGL renderer before any reloads one, and the reloads build a fresh
 * atlas. Reloading one pane at a time would just take the old atlas back from xterm's
 * cache.
 */

/** Twelve 512² pages: below the 16 at which xterm starts merging. */
export const ATLAS_BUDGET_PX = 12 * 512 * 512;

/** A glyph wider than a page gets a max-texture-size page of its own, over budget at
 * once; without a floor, drawing one every frame would reload the renderer every frame. */
export const MIN_RELOAD_INTERVAL_MS = 10_000;

type AtlasAddon = Pick<WebglAddon, 'onAddTextureAtlasCanvas' | 'onRemoveTextureAtlasCanvas'>;

export interface AtlasRenderer {
	/** Dispose the pane's WebGL addon (and unwatch it). */
	drop(): void;
	/** Load a new WebGL addon. */
	load(): void;
}

const renderers = new Set<AtlasRenderer>();
const canvases = new Set<{ width: number; height: number }>();
let reloadQueued = false;
let lastReloadAt = -Infinity;

function atlasPixels(): number {
	let total = 0;
	for (const c of canvases) total += c.width * c.height;
	return total;
}

function reloadAll(): void {
	reloadQueued = false;
	lastReloadAt = Date.now();
	const all = [...renderers];
	for (const r of all) r.drop();
	canvases.clear();
	for (const r of all) r.load();
}

/** Watch `addon`'s atlas pages; call before `terminal.loadAddon(addon)` so its first
 * pages are counted. Returns the unwatch function. */
export function watchAtlas(addon: AtlasAddon, renderer: AtlasRenderer): () => void {
	renderers.add(renderer);
	const added = addon.onAddTextureAtlasCanvas((canvas) => {
		canvases.add(canvas);
		if (reloadQueued || atlasPixels() <= ATLAS_BUDGET_PX) return;
		if (Date.now() - lastReloadAt < MIN_RELOAD_INTERVAL_MS) return;
		reloadQueued = true;
		// Pages are added mid-render; swap renderers once the frame is done.
		setTimeout(reloadAll, 0);
	});
	const removed = addon.onRemoveTextureAtlasCanvas((canvas) => canvases.delete(canvas));
	return () => {
		added.dispose();
		removed.dispose();
		renderers.delete(renderer);
		if (renderers.size === 0) canvases.clear();
	};
}
