import type { ChatImage } from '@workbench/types';

/** Formats the Claude API accepts. */
export const IMAGE_TYPES = ['image/png', 'image/jpeg', 'image/gif', 'image/webp'];
/** The API's per-image limit. */
export const MAX_IMAGE_BYTES = 5 * 1024 * 1024;
export const MAX_IMAGES = 10;

/** A pasted or picked file as a chat image; rejects formats and sizes the API won't take. */
export async function fileToChatImage(file: File): Promise<ChatImage> {
	if (!IMAGE_TYPES.includes(file.type)) {
		throw new Error(`${file.name || 'That file'} isn't a PNG, JPEG, GIF or WebP image.`);
	}
	if (file.size > MAX_IMAGE_BYTES) {
		throw new Error(`${file.name || 'That image'} is over 5 MB.`);
	}
	const bytes = new Uint8Array(await file.arrayBuffer());
	return {
		mediaType: file.type,
		data: toBase64(bytes),
		name: file.name || 'Pasted image'
	};
}

function toBase64(bytes: Uint8Array): string {
	let binary = '';
	const CHUNK = 0x8000; // keep String.fromCharCode's argument list small
	for (let i = 0; i < bytes.length; i += CHUNK) {
		binary += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
	}
	return btoa(binary);
}

/** Image files from a paste or drop, in order. */
export function imageFiles(list: FileList | DataTransferItemList | null | undefined): File[] {
	if (!list) return [];
	const files: File[] = [];
	for (const entry of Array.from(list as ArrayLike<File | DataTransferItem>)) {
		const file = entry instanceof File ? entry : entry.kind === 'file' ? entry.getAsFile() : null;
		if (file && file.type.startsWith('image/')) files.push(file);
	}
	return files;
}

export function previewUrl(image: ChatImage): string {
	return `data:${image.mediaType};base64,${image.data}`;
}
