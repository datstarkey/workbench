import { expect, test } from 'bun:test';
import { PendingAsks } from '../hooks/asks';
import { attachedFiles, midTurn, withAttachments } from '../hooks/lines';

test('identical calls asked in parallel are each found, oldest first', () => {
	const asks = new PendingAsks();
	const input = { command: 'ls' };
	asks.note('Bash', input, { id: 'a' });
	asks.note('Bash', { ...input }, { id: 'b' });
	expect(asks.take('Bash', input)?.id).toBe('a');
	expect(asks.take('Bash', input)?.id).toBe('b');
	expect(asks.take('Bash', input)).toBeUndefined();
});

test('a call settled without a dialog leaves the queue', () => {
	const asks = new PendingAsks();
	asks.note('Bash', {}, { id: 'a' });
	asks.note('Bash', {}, { id: 'b' });
	asks.drop('a');
	expect(asks.take('Bash', {})?.id).toBe('b');
	expect(asks.take('Bash', {})).toBeUndefined();
});

test('a call noted twice is pending once', () => {
	const asks = new PendingAsks();
	asks.note('Bash', {}, { id: 'a' });
	asks.note('Bash', {}, { id: 'a', reason: 'later' });
	expect(asks.take('Bash', {})).toEqual({ id: 'a', reason: 'later' });
	expect(asks.take('Bash', {})).toBeUndefined();
});

test('only the newest pending calls are kept', () => {
	const asks = new PendingAsks();
	for (let i = 0; i < 60; i++) asks.note('Bash', { i }, { id: `c${i}` });
	expect(asks.take('Bash', { i: 0 })).toBeUndefined();
	expect(asks.take('Bash', { i: 9 })).toBeUndefined();
	expect(asks.take('Bash', { i: 10 })?.id).toBe('c10');
	expect(asks.take('Bash', { i: 59 })?.id).toBe('c59');
});

test('only the files the server attached are listed', () => {
	const text = 'use @workbench/ui like @Component does\n\n@/tmp/a.png';
	expect(withAttachments(text, ['/tmp/a.png'])).toBe(
		`${text}\n\nAttached files (read each with the Read tool):\n- /tmp/a.png`
	);
	expect(withAttachments('see @workbench/ui', [])).toBe('see @workbench/ui');
	expect(midTurn('hi @scope/pkg', [])).not.toContain('Attached files');
});

test('without a list from the server every mention counts, as before', () => {
	expect(withAttachments('look @/tmp/a.png and @"/tmp/b c.txt"')).toBe(
		'look @/tmp/a.png and @"/tmp/b c.txt"\n\nAttached files (read each with the Read tool):\n- /tmp/a.png\n- /tmp/b c.txt'
	);
});

test("a prompt line's attachments", () => {
	expect(attachedFiles({ type: 'user' })).toBeUndefined();
	expect(attachedFiles({ type: 'user', workbench_attachments: ['/a', 3, ''] })).toEqual(['/a']);
});
