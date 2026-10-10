import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

type Canvas = { width: number; height: number };
type Listener = (c: Canvas) => void;

/** Stands in for a WebglAddon: only the two atlas events `watchAtlas` reads. */
function fakeAddon() {
	const add = new Set<Listener>();
	const remove = new Set<Listener>();
	const on = (set: Set<Listener>) => (fn: Listener) => {
		set.add(fn);
		return { dispose: () => set.delete(fn) };
	};
	return {
		onAddTextureAtlasCanvas: on(add),
		onRemoveTextureAtlasCanvas: on(remove),
		addPage: (c: Canvas) => add.forEach((fn) => fn(c)),
		removePage: (c: Canvas) => remove.forEach((fn) => fn(c)),
		listeners: () => add.size + remove.size
	};
}

const page = (): Canvas => ({ width: 512, height: 512 });

async function load() {
	vi.resetModules();
	return import('./webgl-atlas');
}

describe('watchAtlas', () => {
	beforeEach(() => vi.useFakeTimers());
	afterEach(() => vi.useRealTimers());

	it('reloads nothing while the atlas stays within budget', async () => {
		const { watchAtlas } = await load();
		const addon = fakeAddon();
		const renderer = { drop: vi.fn(), load: vi.fn() };
		watchAtlas(addon as never, renderer);
		for (let i = 0; i < 12; i++) addon.addPage(page());
		vi.runAllTimers();
		expect(renderer.drop).not.toHaveBeenCalled();
	});

	it('drops every renderer before reloading any once the budget is passed', async () => {
		const { watchAtlas } = await load();
		const calls: string[] = [];
		const a = fakeAddon();
		const b = fakeAddon();
		watchAtlas(a as never, { drop: () => calls.push('drop a'), load: () => calls.push('load a') });
		watchAtlas(b as never, { drop: () => calls.push('drop b'), load: () => calls.push('load b') });
		// The atlas is shared: both addons report the same canvases.
		for (let i = 0; i < 13; i++) {
			const c = page();
			a.addPage(c);
			b.addPage(c);
		}
		expect(calls).toEqual([]); // deferred until the frame is done
		vi.runAllTimers();
		expect(calls).toEqual(['drop a', 'drop b', 'load a', 'load b']);
	});

	it('counts a merged page by its size and forgets removed ones', async () => {
		const { watchAtlas } = await load();
		const addon = fakeAddon();
		const renderer = { drop: vi.fn(), load: vi.fn() };
		watchAtlas(addon as never, renderer);
		const small = Array.from({ length: 4 }, page);
		small.forEach(addon.addPage);
		small.forEach(addon.removePage);
		addon.addPage({ width: 1024, height: 1024 });
		vi.runAllTimers();
		expect(renderer.drop).not.toHaveBeenCalled();
		addon.addPage({ width: 4096, height: 4096 });
		vi.runAllTimers();
		expect(renderer.drop).toHaveBeenCalledOnce();
	});

	it('starts counting afresh after a reload, and waits before the next one', async () => {
		const { watchAtlas, MIN_RELOAD_INTERVAL_MS } = await load();
		const addon = fakeAddon();
		const renderer = { drop: vi.fn(), load: vi.fn() };
		watchAtlas(addon as never, renderer);
		const huge = { width: 16384, height: 16384 };
		addon.addPage(huge);
		vi.runAllTimers();
		expect(renderer.drop).toHaveBeenCalledTimes(1);

		// An oversized glyph drawn again at once: no reload loop.
		addon.addPage({ ...huge });
		vi.runAllTimers();
		expect(renderer.drop).toHaveBeenCalledTimes(1);

		vi.advanceTimersByTime(MIN_RELOAD_INTERVAL_MS);
		addon.addPage({ ...huge });
		vi.runAllTimers();
		expect(renderer.drop).toHaveBeenCalledTimes(2);
	});

	it('unwatching removes the listeners and the renderer', async () => {
		const { watchAtlas } = await load();
		const addon = fakeAddon();
		const renderer = { drop: vi.fn(), load: vi.fn() };
		const unwatch = watchAtlas(addon as never, renderer);
		unwatch();
		expect(addon.listeners()).toBe(0);
		const other = fakeAddon();
		watchAtlas(other as never, { drop: vi.fn(), load: vi.fn() });
		other.addPage({ width: 16384, height: 16384 });
		vi.runAllTimers();
		expect(renderer.drop).not.toHaveBeenCalled();
	});
});
