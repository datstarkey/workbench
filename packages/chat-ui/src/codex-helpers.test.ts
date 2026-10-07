import { describe, expect, it } from 'vitest';
import { decodeAudio, pcm16 } from './codex-helpers';
describe('Codex input helpers', () => {
	it('preserves PCM16 sign and saturation through the audio transport', () => {
		const decoded = decodeAudio(pcm16(new Float32Array([-2, -1, -0.5, 0, 0.5, 1, 2])));
		expect(Array.from(decoded)).toEqual([-1, -1, -0.5, 0, 0.5, 32767 / 32768, 32767 / 32768]);
		expect(() => decodeAudio('AA==')).toThrow('Invalid PCM audio');
	});
});
