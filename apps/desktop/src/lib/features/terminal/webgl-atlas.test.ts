import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import type { Terminal } from '@xterm/xterm';

type Canvas = { width: number; height: number };
type Listener<T> = (v: T) => void;

function emitter<T>() {
	const listeners = new Set<Listener<T>>();
	return {
		event: (fn: Listener<T>) => {
			listeners.add(fn);
			return { dispose: () => listeners.delete(fn) };
		},
		fire: (v: T) => listeners.forEach((fn) => fn(v)),
		clear: () => listeners.clear()
	};
}

/** Stands in for WebglAddon: the atlas and context-loss events, and dispose. */
class FakeAddon {
	static all: FakeAddon[] = [];
	static log: string[] = [];
	static failNext = false;
	change = emitter<Canvas>();
	add = emitter<Canvas>();
	remove = emitter<Canvas>();
	lost = emitter<void>();
	onChangeTextureAtlas = this.change.event;
	onAddTextureAtlasCanvas = this.add.event;
	onRemoveTextureAtlasCanvas = this.remove.event;
	onContextLoss = this.lost.event;
	disposed = false;
	constructor() {
		if (FakeAddon.failNext) {
			FakeAddon.failNext = false;
			throw new Error('WebGL2 not supported');
		}
		FakeAddon.all.push(this);
		FakeAddon.log.push('load');
	}
	dispose() {
		this.disposed = true;
		FakeAddon.log.push('drop');
		for (const e of [this.change, this.add, this.remove, this.lost]) e.clear();
	}
}

const page = (width = 512): Canvas => ({ width, height: width });
const terminal = { loadAddon: vi.fn() } as unknown as Terminal;

async function load() {
	vi.resetModules();
	vi.doMock('@xterm/addon-webgl', () => ({ WebglAddon: FakeAddon }));
	FakeAddon.all = [];
	FakeAddon.log = [];
	return import('./webgl-atlas');
}

/** Atlas pages of a shared atlas, reported by every renderer using it. */
function addShared(addons: FakeAddon[], canvas: Canvas) {
	for (const a of addons) a.add.fire(canvas);
}

describe('WebglRenderer', () => {
	beforeEach(() => vi.useFakeTimers());
	afterEach(() => vi.useRealTimers());

	it('keeps its renderer while the atlas stays within budget', async () => {
		const { WebglRenderer, ATLAS_PAGE_BUDGET } = await load();
		new WebglRenderer(terminal, vi.fn()).load();
		const [addon] = FakeAddon.all;
		addon.change.fire(page());
		for (let i = 1; i < ATLAS_PAGE_BUDGET; i++) addon.add.fire(page());
		vi.runAllTimers();
		expect(addon.disposed).toBe(false);
	});

	it('drops every renderer before reloading any once past the budget', async () => {
		const { WebglRenderer, ATLAS_PAGE_BUDGET } = await load();
		new WebglRenderer(terminal, vi.fn()).load();
		new WebglRenderer(terminal, vi.fn()).load();
		const [a, b] = FakeAddon.all;
		const first = page();
		a.change.fire(first);
		b.change.fire(first);
		for (let i = 0; i < ATLAS_PAGE_BUDGET; i++) addShared([a, b], page());
		expect(a.disposed).toBe(false); // deferred until the frame is done
		vi.runAllTimers();
		expect(FakeAddon.log).toEqual(['load', 'load', 'drop', 'drop', 'load', 'load']);
	});

	it("ignores xterm's fixed overflow page", async () => {
		const { WebglRenderer } = await load();
		new WebglRenderer(terminal, vi.fn()).load();
		const [addon] = FakeAddon.all;
		addon.change.fire(page());
		addon.add.fire(page(16384));
		vi.runAllTimers();
		expect(addon.disposed).toBe(false);
	});

	it('forgets the old atlas when it moves to a new one', async () => {
		const { WebglRenderer, ATLAS_PAGE_BUDGET } = await load();
		new WebglRenderer(terminal, vi.fn()).load();
		const [addon] = FakeAddon.all;
		addon.change.fire(page());
		for (let i = 1; i < ATLAS_PAGE_BUDGET; i++) addon.add.fire(page());
		// e.g. a DPR change: a new atlas, and no remove events for the old pages
		addon.change.fire(page());
		addon.add.fire(page());
		vi.runAllTimers();
		expect(addon.disposed).toBe(false);
	});

	it('counts removed pages out', async () => {
		const { WebglRenderer, ATLAS_PAGE_BUDGET } = await load();
		new WebglRenderer(terminal, vi.fn()).load();
		const [addon] = FakeAddon.all;
		addon.change.fire(page());
		const pages = Array.from({ length: ATLAS_PAGE_BUDGET - 1 }, () => page());
		pages.forEach((p) => addon.add.fire(p));
		pages.forEach((p) => addon.remove.fire(p));
		addon.add.fire(page());
		vi.runAllTimers();
		expect(addon.disposed).toBe(false);
	});

	it('drops itself on context loss and tells the pane', async () => {
		const { WebglRenderer, ATLAS_PAGE_BUDGET } = await load();
		const onContextLoss = vi.fn();
		new WebglRenderer(terminal, onContextLoss).load();
		const [lost] = FakeAddon.all;
		lost.lost.fire();
		expect(lost.disposed).toBe(true);
		expect(onContextLoss).toHaveBeenCalledOnce();

		// A dropped renderer is out of later reloads.
		new WebglRenderer(terminal, vi.fn()).load();
		const live = FakeAddon.all[1];
		live.change.fire(page());
		for (let i = 0; i < ATLAS_PAGE_BUDGET; i++) live.add.fire(page());
		vi.runAllTimers();
		expect(FakeAddon.all).toHaveLength(3);
	});

	it('keeps the DOM renderer when WebGL fails, and can load later', async () => {
		const { WebglRenderer } = await load();
		FakeAddon.failNext = true;
		const renderer = new WebglRenderer(terminal, vi.fn());
		expect(() => renderer.load()).not.toThrow();
		expect(FakeAddon.all).toHaveLength(0);
		renderer.load();
		expect(FakeAddon.all).toHaveLength(1);
	});
});
