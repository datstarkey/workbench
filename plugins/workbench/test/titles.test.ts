import { expect, test } from 'bun:test';
import { Titles } from '../hooks/titles';

const answer = (text: string) => async () => text;

test('an answered turn names the session from its prompts and reply', async () => {
	const titles = new Titles();
	titles.notePrompt('the chat freezes after a tool call');
	titles.noteReply('Found it: a duplicate menu key.');
	let asked = '';
	const title = await titles.retitle('the chat freezes after a', async (prompt) => {
		asked = prompt;
		return '"Fix chat freezing after tool calls."\nextra';
	});
	expect(title).toBe('Fix chat freezing after tool calls');
	expect(asked).toContain('Current title: the chat freezes after a');
	expect(asked).toContain('Opening request:\nthe chat freezes after a tool call');
	expect(asked).toContain('Latest reply:\nFound it');
	expect(titles.takePending()).toBe(title);
	expect(titles.takePending()).toBeUndefined();
});

test('only a turn with a new prompt asks again, and an unchanged title is no title', async () => {
	const titles = new Titles();
	expect(await titles.retitle(undefined, answer('A'))).toBeUndefined();
	titles.notePrompt('first');
	expect(await titles.retitle('Same', answer('Same'))).toBeUndefined();
	expect(await titles.retitle('Same', answer('Other'))).toBeUndefined();
	titles.notePrompt('<command-name>/cost</command-name>');
	expect(await titles.retitle('Same', answer('Other'))).toBeUndefined();
	titles.notePrompt('second');
	expect(await titles.retitle('Same', answer('Other'))).toBe('Other');
});

test('a rename by a person stops automatic titles until the conversation changes', async () => {
	const titles = new Titles();
	titles.notePrompt('first');
	titles.nameByPerson();
	expect(await titles.retitle(undefined, answer('Auto'))).toBeUndefined();
	titles.reset();
	titles.notePrompt('a new conversation');
	expect(await titles.retitle(undefined, answer('Auto'))).toBe('Auto');
});

test('a rename while the model answers wins', async () => {
	const titles = new Titles();
	titles.notePrompt('first');
	const late = titles.retitle(undefined, async () => {
		titles.nameByPerson();
		return 'Auto';
	});
	expect(await late).toBeUndefined();
	expect(titles.takePending()).toBeUndefined();
});

test('a failed completion keeps the title', async () => {
	const titles = new Titles();
	titles.notePrompt('first');
	expect(await titles.retitle('Kept', () => Promise.reject(new Error('blocked')))).toBeUndefined();
});
