import { describe, expect, it } from 'vitest';
import { parseTerminalControlFrame } from './terminal-frames.ts';

describe('parseTerminalControlFrame', () => {
	it('parses a takeover frame', () => {
		expect(parseTerminalControlFrame('{"t":"takeover"}')).toEqual({ t: 'takeover' });
	});

	it('parses an exit frame with and without a code', () => {
		expect(parseTerminalControlFrame('{"t":"exit","code":2}')).toEqual({ t: 'exit', code: 2 });
		expect(parseTerminalControlFrame('{"t":"exit","code":null}')).toEqual({
			t: 'exit',
			code: null
		});
		expect(parseTerminalControlFrame('{"t":"exit"}')).toEqual({ t: 'exit', code: null });
	});

	it('returns null for non-control text', () => {
		for (const data of ['hello', '', '42', 'null', '[]', '{"t":"i","d":"x"}', '{}']) {
			expect(parseTerminalControlFrame(data)).toBeNull();
		}
	});
});
