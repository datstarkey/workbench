import { expect, it, vi } from 'vitest';
import { Dictation, insertDictation } from './dictation.svelte';

it('inserts speech into an editable draft or replaces the selection', () => {
	expect(insertDictation('', '  Hello world  ')).toEqual({ text: 'Hello world', caret: 11 });
	expect(insertDictation('Fix the bug', 'and add tests').text).toBe('Fix the bug and add tests');
	expect(insertDictation('Fix\n', 'the bug').text).toBe('Fix\nthe bug');
	expect(insertDictation('Fix this bug.', 'the crash', 4, 12)).toEqual({
		text: 'Fix the crash.',
		caret: 13
	});
	expect(insertDictation('Fix bug', 'the', 4).text).toBe('Fix the bug');
});

it('prevents overlapping requests and inserts text without sending', async () => {
	let finish!: (text: string) => void;
	const recognize = vi.fn(() => new Promise<string>((resolve) => (finish = resolve)));
	const voice = new Dictation(recognize);
	const insert = vi.fn();
	const pending = voice.start(insert);
	expect(voice.busy).toBe(true);
	await voice.start(insert);
	expect(recognize).toHaveBeenCalledTimes(1);
	finish(' Hello ');
	await pending;
	expect(insert).toHaveBeenCalledExactlyOnceWith('Hello');
	expect(voice.busy).toBe(false);
});

it('leaves the draft alone on cancellation or empty results', async () => {
	const insert = vi.fn();
	const recognize = vi.fn().mockResolvedValueOnce(null).mockResolvedValueOnce(' ');
	const voice = new Dictation(recognize);
	await voice.start(insert);
	await voice.start(insert);
	expect(insert).not.toHaveBeenCalled();
	expect(voice.error).toBeNull();
});

it('shows native failures and clears the error on retry', async () => {
	const recognize = vi.fn().mockRejectedValueOnce('No recognizer').mockResolvedValueOnce('hello');
	const voice = new Dictation(recognize);
	await voice.start(vi.fn());
	expect(voice.error).toBe('No recognizer');
	expect(voice.busy).toBe(false);
	const insert = vi.fn();
	await voice.start(insert);
	expect(voice.error).toBeNull();
	expect(insert).toHaveBeenCalledWith('hello');
});

it('discards speech that arrives after leaving the conversation', async () => {
	let finish!: (text: string) => void;
	const voice = new Dictation(() => new Promise<string>((resolve) => (finish = resolve)));
	const insert = vi.fn();
	const pending = voice.start(insert);
	voice.dispose();
	finish('old conversation');
	await pending;
	await voice.start(insert);
	expect(insert).not.toHaveBeenCalled();
});
