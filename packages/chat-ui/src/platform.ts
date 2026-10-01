import { getContext, setContext } from 'svelte';
import type { ChatImage } from '@workbench/types';

/** What the chat views need from the app hosting them. */
export interface ChatPlatform {
	/** Open an http(s) or mailto link outside the app. */
	openLink(href: string): void;
	/** Enter sends and Shift+Enter breaks the line (default). Phones set false: no Shift, so Enter breaks and the button sends. */
	enterSends?: boolean;
	/**
	 * Desktop: the webview takes OS file drops before the page sees them, so the
	 * host watches drops over `node` and hands over the images it read.
	 */
	watchImageDrops?(
		node: HTMLElement,
		on: { hover(over: boolean): void; drop(images: ChatImage[], error: string | null): void }
	): () => void;
}

const KEY = Symbol('chat-platform');

const browser: ChatPlatform = {
	openLink: (href) => void window.open(href, '_blank', 'noopener')
};

/** Call during the host component's init; chat components below it use it. */
export function setChatPlatform(platform: ChatPlatform): void {
	setContext(KEY, platform);
}

export function getChatPlatform(): ChatPlatform {
	return getContext<ChatPlatform | undefined>(KEY) ?? browser;
}
