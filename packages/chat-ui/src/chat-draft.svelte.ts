import type { ChatFile, ChatImage } from '@workbench/types';

/** What the composer holds before it's sent; outlives the view that edits it. */
export class ChatDraft {
	text = $state('');
	images = $state<ChatImage[]>([]);
	files = $state<ChatFile[]>([]);
	constructor(text = '') {
		this.text = text;
	}
}
