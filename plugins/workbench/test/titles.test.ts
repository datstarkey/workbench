import { expect, test } from 'bun:test';
import { clean, Titles, type Store } from '../hooks/titles';

const answer = (text: string) => async () => text;

function memoryStore(): Store & { data: Map<string, unknown> } {
	const data = new Map<string, unknown>();
	return {
		data,
		get: async (k) => structuredClone(data.get(k)),
		set: async (k, v) => void data.set(k, structuredClone(v)),
		keys: async () => [...data.keys()],
		delete: async (k) => void data.delete(k)
	};
}

async function session(store = memoryStore(), id = 's1') {
	const titles = new Titles();
	await titles.use(id, store.get);
	return { titles, store };
}

test('an answered turn names the session from its prompts and reply', async () => {
	const { titles, store } = await session();
	titles.notePrompt('the chat freezes after a tool call');
	titles.noteReply('Found it: a duplicate menu key.');
	let asked = '';
	const title = await titles.retitle(
		'the chat freezes after a',
		async (prompt) => {
			asked = prompt;
			return '“Fix chat freezing after tool calls.”\nextra';
		},
		store
	);
	expect(title).toBe('Fix chat freezing after tool calls');
	expect(asked).toContain('Current title: the chat freezes after a');
	expect(asked).toContain('Opening request:\nthe chat freezes after a tool call');
	expect(asked).toContain('Latest reply:\nFound it');
	expect(titles.takePending()).toBe(title);
	expect(titles.takePending()).toBeUndefined();
});

test('names on the first prompts, then every fifth', async () => {
	const { titles, store } = await session();
	const asked: number[] = [];
	for (let n = 1; n <= 10; n++) {
		titles.notePrompt(`prompt ${n}`);
		if (await titles.retitle('Old', answer(`Title ${n}`), store)) asked.push(n);
	}
	expect(asked).toEqual([1, 2, 3, 5, 10]);
});

test('an unchanged title, or no new prompt, is no title', async () => {
	const { titles, store } = await session();
	expect(await titles.retitle(undefined, answer('A'), store)).toBeUndefined();
	titles.notePrompt('first');
	expect(await titles.retitle('Same', answer('Same'), store)).toBeUndefined();
	expect(await titles.retitle('Same', answer('Other'), store)).toBeUndefined();
	titles.notePrompt('<command-name>/cost</command-name>');
	expect(await titles.retitle('Same', answer('Other'), store)).toBeUndefined();
});

test("a person's rename and the opening request survive a restart", async () => {
	const store = memoryStore();
	const before = (await session(store)).titles;
	before.notePrompt('migrate auth to OAuth');
	await before.retitle(undefined, answer('Migrate auth to OAuth'), store);

	const after = (await session(store)).titles;
	after.notePrompt('also bump the lockfile');
	let asked = '';
	await after.retitle(
		'Migrate auth to OAuth',
		async (p) => {
			asked = p;
			return 'Migrate auth to OAuth';
		},
		store
	);
	expect(asked).toContain('Opening request:\nmigrate auth to OAuth');

	await after.nameByPerson(store);
	const renamed = (await session(store)).titles;
	renamed.notePrompt('more');
	expect(await renamed.retitle(undefined, answer('Auto'), store)).toBeUndefined();
});

test('a title asked before the conversation changed is dropped', async () => {
	const { titles, store } = await session();
	titles.notePrompt('first');
	const late = titles.retitle(
		undefined,
		async () => {
			titles.reset();
			return 'Auto';
		},
		store
	);
	expect(await late).toBeUndefined();
	expect(titles.takePending()).toBeUndefined();
});

test('a rename while the model answers wins', async () => {
	const { titles, store } = await session();
	titles.notePrompt('first');
	const late = titles.retitle(
		undefined,
		async () => {
			await titles.nameByPerson(store);
			return 'Auto';
		},
		store
	);
	expect(await late).toBeUndefined();
});

test('a failed completion keeps the title', async () => {
	const { titles, store } = await session();
	titles.notePrompt('first');
	const failed = () => Promise.reject(new Error('blocked'));
	expect(await titles.retitle('Kept', failed, store)).toBeUndefined();
});

test('only the newest sessions are kept in the store', async () => {
	const store = memoryStore();
	for (let i = 0; i < 205; i++) {
		const { titles } = await session(store, `s${i}`);
		titles.notePrompt('p');
		await titles.retitle(undefined, answer('T'), store);
	}
	expect(store.data.size).toBe(200);
	expect(store.data.has('title:s0')).toBe(false);
	expect(store.data.has('title:s204')).toBe(true);
});

test('a title is cleaned to one short line', () => {
	expect(clean(' "Fix the bug." ')).toBe('Fix the bug');
	expect(clean("Update users'")).toBe("Update users'");
	expect(clean('')).toBeUndefined();
	const long = clean(
		'Refactor the authentication middleware to support multiple identity providers'
	);
	expect(long).toBe('Refactor the authentication middleware to support multiple');
});
