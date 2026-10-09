import { afterEach, describe, expect, it, vi } from 'vitest';
import { createHttpTransport } from '@workbench/transport';
import type { HostUpdateStatus } from '@workbench/types';
import { HostUpdate } from './host-update.svelte.ts';
import { jsonResponse, routeFetch, TOKEN } from './test-helpers.ts';

const BASE = 'http://box:4317';

/** A host whose `GET /host/update` answers `state.status`; POST answers `post`. */
function host(status: HostUpdateStatus | number, post = 202) {
	const state = { status };
	const spy = routeFetch({
		'/host/update': (init) => {
			if (init?.method === 'POST')
				return post === 202
					? jsonResponse({ version: '1.1.0' }, 202)
					: jsonResponse({ error: 'no update available' }, post);
			return typeof state.status === 'number'
				? jsonResponse({ error: 'nope' }, state.status)
				: jsonResponse(state.status);
		}
	});
	return { state, spy };
}

const OLD: HostUpdateStatus = { current: '1.0.0', available: '1.1.0', installing: false };

function hostUpdate(): HostUpdate {
	return new HostUpdate(createHttpTransport({ baseUrl: BASE, token: TOKEN }));
}

/** Checked, then installing 1.1.0. */
async function installing(): Promise<{ update: HostUpdate } & ReturnType<typeof host>> {
	const h = host(OLD);
	const update = hostUpdate();
	await update.check();
	await update.install();
	return { update, ...h };
}

afterEach(() => vi.unstubAllGlobals());

describe('HostUpdate', () => {
	it("shows the host's version and its update", async () => {
		const { spy } = host(OLD);
		const update = hostUpdate();
		await update.check();
		expect(update.status).toEqual(OLD);
		expect(update.updating).toBe(false);
		const [url, init] = spy.mock.calls[0];
		expect(url).toBe(`${BASE}/host/update`);
		expect((init?.headers as Record<string, string>).authorization).toBe(`Bearer ${TOKEN}`);
	});

	it('stays hidden on a host that cannot update itself (501)', async () => {
		const { state } = host(OLD);
		const update = hostUpdate();
		await update.check();
		state.status = 501;
		await update.check();
		expect(update.status).toBeNull();
	});

	it('is updating from the 202 until the host answers on another version', async () => {
		const { update, state, spy } = await installing();
		expect(spy.mock.lastCall?.[1]?.method).toBe('POST');
		expect(spy.mock.lastCall?.[1]?.body).toBe(JSON.stringify({ version: '1.1.0' }));
		expect(update.updating).toBe(true);
		expect(update.target).toBe('1.1.0');

		state.status = { ...OLD, available: null, installing: true };
		await update.check();
		expect(update.updating).toBe(true);

		// Restarting: unreachable is not a failure.
		spy.mockRejectedValueOnce(new TypeError('Failed to fetch'));
		await update.check();
		expect(update.updating).toBe(true);

		// Any new version is success, even one newer than the 202 named.
		state.status = { current: '1.2.0', available: null, installing: false };
		await update.check();
		expect(update.updating).toBe(false);
		expect(update.error).toBeNull();
		expect(update.status?.current).toBe('1.2.0');
	});

	it('reports an install the host gave up on', async () => {
		const { update } = await installing();
		await update.check(); // installing: false, still 1.0.0
		expect(update.updating).toBe(false);
		expect(update.error).toMatch(/couldn't install/);
	});

	it("doesn't let a check sent before the install settle it", async () => {
		const { spy } = host(OLD);
		const update = hostUpdate();
		await update.check();
		let reply!: (r: Response) => void;
		spy.mockImplementationOnce(() => new Promise((resolve) => (reply = resolve)));
		const stale = update.check();
		await update.install();
		reply(jsonResponse(OLD));
		await stale;
		expect(update.updating).toBe(true);
		expect(update.error).toBeNull();
	});

	it('shows an install started elsewhere as updating', async () => {
		host({ ...OLD, available: null, installing: true });
		const update = hostUpdate();
		await update.check();
		expect(update.target).toBeNull();
		expect(update.updating).toBe(true);
	});

	it('shows why the host refused to install, until a check succeeds', async () => {
		host(OLD, 409);
		const update = hostUpdate();
		await update.check();
		await update.install();
		expect(update.updating).toBe(false);
		expect(update.error).toMatch(/^Couldn't update the host: .*no update available/);
		await update.check();
		expect(update.error).toBeNull();
	});

	it('does nothing without an available update', async () => {
		const { spy } = host({ ...OLD, available: null });
		const update = hostUpdate();
		await update.check();
		await update.install();
		expect(spy).toHaveBeenCalledTimes(1);
	});
});
