/** Small, platform-neutral helpers for native Codex controls. */
export function safeExternalUrl(value: unknown): string | null {
	if (typeof value !== 'string') return null;
	try {
		const url = new URL(value);
		return url.protocol === 'https:' || url.protocol === 'http:' ? url.href : null;
	} catch {
		return null;
	}
}

export function pcm16(samples: Float32Array): string {
	const bytes = new Uint8Array(samples.length * 2);
	const view = new DataView(bytes.buffer);
	for (let i = 0; i < samples.length; i++) {
		const v = Math.max(-1, Math.min(1, samples[i]));
		view.setInt16(i * 2, Math.round(v < 0 ? v * 32768 : v * 32767), true);
	}
	return btoa(Array.from(bytes, (v) => String.fromCharCode(v)).join(''));
}

export function decodeAudio(data: string): Float32Array {
	const bytes = Uint8Array.from(atob(data), (c) => c.charCodeAt(0));
	if (bytes.length % 2) throw new Error('Invalid PCM audio');
	const view = new DataView(bytes.buffer);
	return Float32Array.from(
		{ length: bytes.length / 2 },
		(_, i) => view.getInt16(i * 2, true) / 32768
	);
}
