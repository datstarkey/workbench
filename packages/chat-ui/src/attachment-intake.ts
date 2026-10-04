import type { ChatAttachment, ChatFile, ChatImage } from '@workbench/types';

/** Formats the Claude API accepts. */
export const IMAGE_TYPES = ['image/png', 'image/jpeg', 'image/gif', 'image/webp'];
/** The API's per-image limit. */
export const MAX_IMAGE_BYTES = 5 * 1024 * 1024;
export const MAX_IMAGES = 10;
/** Mirrors `workbench_core::chat_attachment`. */
export const MAX_PDF_BYTES = 10 * 1024 * 1024;
export const MAX_TEXT_BYTES = 256 * 1024;
export const MAX_FILES = 5;
export const PDF_TYPE = 'application/pdf';

/** What a composer holds: images, plus PDFs and text files where the agent reads them. */
export interface Attachments {
	images: ChatImage[];
	files: ChatFile[];
}

/** A pasted or picked file as a chat image; rejects formats and sizes the API won't take. */
export async function fileToChatImage(file: File): Promise<ChatImage> {
	if (!IMAGE_TYPES.includes(file.type)) {
		throw new Error(`${file.name || 'That file'} isn't a PNG, JPEG, GIF or WebP image.`);
	}
	if (file.size > MAX_IMAGE_BYTES) {
		throw new Error(`${file.name || 'That image'} is over 5 MB.`);
	}
	return {
		mediaType: file.type,
		data: toBase64(await bytesOf(file)),
		name: file.name || 'Pasted image'
	};
}

/**
 * A pasted, picked or dropped file as an attachment. With `documents` (Claude)
 * a PDF or any UTF-8 text file is taken too; otherwise images only.
 */
export async function fileToAttachment(file: File, documents: boolean): Promise<ChatAttachment> {
	if (IMAGE_TYPES.includes(file.type) || !documents) {
		return { kind: 'image', ...(await fileToChatImage(file)) };
	}
	const name = file.name || 'Pasted file';
	if (file.type === PDF_TYPE || /\.pdf$/i.test(name)) {
		if (file.size > MAX_PDF_BYTES) throw new Error(`${name} is over 10 MB.`);
		return { kind: 'file', mediaType: PDF_TYPE, data: toBase64(await bytesOf(file)), name };
	}
	const unsupported = new Error(`${name} isn't an image, PDF or text file.`);
	if (file.size > MAX_TEXT_BYTES) {
		throw file.type.startsWith('text/') ? new Error(`${name} is over 256 KB.`) : unsupported;
	}
	const bytes = await bytesOf(file);
	if (bytes.includes(0)) throw unsupported;
	try {
		const text = new TextDecoder('utf-8', { fatal: true }).decode(bytes);
		return { kind: 'file', mediaType: 'text/plain', data: text, name };
	} catch {
		throw unsupported;
	}
}

/**
 * Add what was read to what's attached, within the per-message caps. Files an
 * agent can't read (`documents` false) are refused with a note, not dropped silently.
 */
export function addAttachments(
	current: Attachments,
	added: ChatAttachment[],
	documents: boolean
): Attachments & { error: string | null } {
	const images = added.flatMap(({ kind, ...a }) => (kind === 'image' ? [a as ChatImage] : []));
	const files = added.flatMap(({ kind, ...a }) => (kind === 'file' ? [a as ChatFile] : []));
	let error: string | null = null;
	if (files.length > 0 && !documents) error = 'Only images can be attached here.';
	const imageRoom = Math.max(0, MAX_IMAGES - current.images.length);
	const fileRoom = documents ? Math.max(0, MAX_FILES - current.files.length) : 0;
	if (images.length > imageRoom) error = `Attach up to ${MAX_IMAGES} images per message.`;
	if (documents && files.length > fileRoom) error = `Attach up to ${MAX_FILES} files per message.`;
	return {
		images: [...current.images, ...images.slice(0, imageRoom)],
		files: [...current.files, ...files.slice(0, fileRoom)],
		error
	};
}

async function bytesOf(file: File): Promise<Uint8Array> {
	return new Uint8Array(await file.arrayBuffer());
}

function toBase64(bytes: Uint8Array): string {
	let binary = '';
	const CHUNK = 0x8000; // keep String.fromCharCode's argument list small
	for (let i = 0; i < bytes.length; i += CHUNK) {
		binary += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
	}
	return btoa(binary);
}

/** The files in a paste, drop or picker, in order. */
export function filesIn(list: FileList | DataTransferItemList | null | undefined): File[] {
	if (!list) return [];
	const files: File[] = [];
	for (const entry of Array.from(list as ArrayLike<File | DataTransferItem>)) {
		const file = entry instanceof File ? entry : entry.kind === 'file' ? entry.getAsFile() : null;
		if (file) files.push(file);
	}
	return files;
}

export function previewUrl(image: ChatImage): string {
	return `data:${image.mediaType};base64,${image.data}`;
}
