import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { Drafts } from './drafts.svelte';
import { SavedMachines } from './machines.svelte';
import { stubLocalStorage } from './test-helpers';

beforeEach(stubLocalStorage);
afterEach(() => vi.unstubAllGlobals());
const ref = { sessionId: 's', projectPath: '/repo', name: 'repo' };

it('retains text across relaunch and images while navigating', () => {
	const drafts = new Drafts('mac');
	const draft = drafts.get(ref);
	draft.text = 'unfinished';
	draft.images = [{ name: 'image.png', mediaType: 'image/png', data: 'AA==' }];
	drafts.save(ref, draft);
	expect(drafts.get(ref).images).toHaveLength(1);
	expect(new Drafts('mac').get(ref).text).toBe('unfinished');
	expect(new Drafts('pc').get(ref).text).toBe('');
	expect(drafts.get({ ...ref, claudeAccountId: 'work' }).text).toBe('');
});

it('moves drafts when a thread starts or clears, and removes sent text', () => {
	const drafts = new Drafts('mac');
	const draft = drafts.get(ref);
	draft.text = 'next message';
	drafts.save(ref, draft);
	const next = { ...ref, sessionId: 'next' };
	drafts.move(ref, next, draft);
	expect(new Drafts('mac').get(ref).text).toBe('');
	expect(new Drafts('mac').get(next).text).toBe('next message');
	draft.text = '';
	drafts.save(next, draft);
	expect(new Drafts('mac').get(next).text).toBe('');
});

it('forgetting a machine clears its drafts while preserving the other machine', () => {
	const machines = new SavedMachines();
	const mac = machines.save('http://mac', 'mac-token');
	const pc = machines.save('http://pc', 'pc-token');
	for (const m of [mac, pc]) {
		const drafts = new Drafts(m.id);
		const d = drafts.get(ref);
		d.text = m.name;
		drafts.save(ref, d);
	}
	machines.remove(mac.id);
	expect(new Drafts(mac.id).get(ref).text).toBe('');
	expect(new Drafts(pc.id).get(ref).text).toBe('pc');
});
