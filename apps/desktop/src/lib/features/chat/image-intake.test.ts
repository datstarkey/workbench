import { describe, expect, it } from 'vitest';
import { fileToChatImage, imageFiles, previewUrl } from './image-intake';

describe('image intake', () => {
	it('encodes a supported image as base64 without the data URL prefix', async () => {
		const file = new File([new Uint8Array([0x89, 0x50, 0x4e, 0x47])], 'shot.png', {
			type: 'image/png'
		});
		const image = await fileToChatImage(file);
		expect(image).toEqual({ mediaType: 'image/png', data: 'iVBORw==', name: 'shot.png' });
		expect(previewUrl(image)).toBe('data:image/png;base64,iVBORw==');
	});

	it('refuses formats and sizes the API will not take', async () => {
		await expect(
			fileToChatImage(new File(['x'], 'a.svg', { type: 'image/svg+xml' }))
		).rejects.toThrow(/PNG, JPEG, GIF or WebP/);
		const big = new File([new Uint8Array(5 * 1024 * 1024 + 1)], 'big.png', { type: 'image/png' });
		await expect(fileToChatImage(big)).rejects.toThrow(/over 5 MB/);
	});

	it('keeps only image files from a paste', () => {
		const files = [
			new File(['a'], 'a.png', { type: 'image/png' }),
			new File(['b'], 'b.txt', { type: 'text/plain' })
		];
		const list = Object.assign(files, { item: (i: number) => files[i] }) as unknown as FileList;
		expect(imageFiles(list).map((f) => f.name)).toEqual(['a.png']);
		expect(imageFiles(null)).toEqual([]);
	});
});
