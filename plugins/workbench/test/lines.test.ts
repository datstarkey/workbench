import { expect, test } from 'bun:test';
import { commandRowOutput } from '../hooks/lines';

test("a command's output row gives what it printed", () => {
	expect(
		commandRowOutput('<local-command-stdout>Goal set: tests pass</local-command-stdout>')
	).toBe('Goal set: tests pass');
	expect(commandRowOutput('<local-command-stderr>\nNo goal set\n</local-command-stderr>')).toBe(
		'No goal set'
	);
});

test('its name row, an empty output and mismatched tags give nothing', () => {
	expect(commandRowOutput('<command-name>/goal</command-name>')).toBeUndefined();
	expect(commandRowOutput('<local-command-stdout></local-command-stdout>')).toBeUndefined();
	expect(commandRowOutput('<local-command-stdout>x</local-command-stderr>')).toBeUndefined();
});
