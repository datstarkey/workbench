import { describe, expect, it } from 'vitest';
import type { ChatAttachment } from '@workbench/types';
import {
	addAttachments,
	fileToAttachment,
	fileToChatImage,
	filesIn,
	MAX_FILES,
	MAX_IMAGES,
	MAX_TEXT_BYTES,
	previewUrl
} from './attachment-intake';

describe('attachment intake', () => {
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

	it('reads PDFs as base64 and text files (any extension) as text', async () => {
		const pdf = new File(['%PDF'], 'report.pdf', { type: '' });
		expect(await fileToAttachment(pdf, true)).toEqual({
			kind: 'file',
			mediaType: 'application/pdf',
			data: 'JVBERg==',
			name: 'report.pdf'
		});
		const code = new File(['fn main() {} // ü'], 'main.rs', { type: '' });
		expect(await fileToAttachment(code, true)).toEqual({
			kind: 'file',
			mediaType: 'text/plain',
			data: 'fn main() {} // ü',
			name: 'main.rs'
		});
		const png = new File([new Uint8Array([0x89, 0x50, 0x4e, 0x47])], 'a.png', {
			type: 'image/png'
		});
		expect((await fileToAttachment(png, true)).kind).toBe('image');
	});

	it('refuses binaries, oversized text, and documents where only images go', async () => {
		const binary = new File([new Uint8Array([0x7f, 0x45, 0, 1])], 'app', { type: '' });
		await expect(fileToAttachment(binary, true)).rejects.toThrow(/isn't an image, PDF or text/);
		const latin1 = new File([new Uint8Array([0x63, 0xe9])], 'old.txt', { type: 'text/plain' });
		await expect(fileToAttachment(latin1, true)).rejects.toThrow(/isn't an image, PDF or text/);
		const long = new File(['a'.repeat(MAX_TEXT_BYTES + 1)], 'big.log', { type: 'text/plain' });
		await expect(fileToAttachment(long, true)).rejects.toThrow(/over 256 KB/);
		const pdf = new File(['%PDF'], 'report.pdf', { type: 'application/pdf' });
		await expect(fileToAttachment(pdf, false)).rejects.toThrow(/PNG, JPEG, GIF or WebP/);
	});

	it('adds within the per-message caps and says what was left out', () => {
		const image: ChatAttachment = { kind: 'image', mediaType: 'image/png', data: 'x', name: 'a' };
		const file: ChatAttachment = { kind: 'file', mediaType: 'text/plain', data: 'x', name: 'f' };
		const none = { images: [], files: [] };

		const both = addAttachments(none, [image, file], true);
		expect(both).toEqual({
			images: [{ mediaType: 'image/png', data: 'x', name: 'a' }],
			files: [{ mediaType: 'text/plain', data: 'x', name: 'f' }],
			error: null
		});

		const tooMany = addAttachments(none, Array(MAX_FILES + 1).fill(file), true);
		expect(tooMany.files).toHaveLength(MAX_FILES);
		expect(tooMany.error).toMatch(/up to 5 files/);
		expect(addAttachments(none, Array(MAX_IMAGES + 1).fill(image), true).error).toMatch(
			/up to 10 images/
		);

		const imagesOnly = addAttachments(none, [image, file], false);
		expect(imagesOnly.files).toEqual([]);
		expect(imagesOnly.images).toHaveLength(1);
		expect(imagesOnly.error).toMatch(/Only images/);
	});

	it('takes every file from a paste or drop', () => {
		const files = [
			new File(['a'], 'a.png', { type: 'image/png' }),
			new File(['b'], 'b.txt', { type: 'text/plain' })
		];
		const list = Object.assign(files, { item: (i: number) => files[i] }) as unknown as FileList;
		expect(filesIn(list).map((f) => f.name)).toEqual(['a.png', 'b.txt']);
		expect(filesIn(null)).toEqual([]);
	});
});
