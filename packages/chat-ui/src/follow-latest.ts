import { watch } from 'runed';
import type { Attachment } from 'svelte/attachments';
import type { AgentChat } from './agent-chat.svelte';

/** How close to the bottom still counts as reading the newest message. */
const STICK_PX = 48;

/**
 * Keep the newest message in view unless the reader scrolled up; sending a
 * prompt brings it back. `viewport` picks the element that actually scrolls
 * (e.g. the one a custom scrollbar library generates inside the host).
 */
export function followLatest(
	chat: Pick<AgentChat, 'items' | 'pending' | 'now' | 'sends'>,
	viewport: (host: HTMLElement) => HTMLElement = (host) => host
): Attachment<HTMLElement> {
	return (host) => {
		const node = viewport(host);
		let stick = true;
		const onScroll = () => {
			stick = node.scrollHeight - node.scrollTop - node.clientHeight < STICK_PX;
		};
		node.addEventListener('scroll', onScroll);
		watch(
			() => chat.sends,
			() => {
				stick = true;
			},
			{ lazy: true }
		);
		watch(
			() => [chat.items, chat.pending, chat.now.kind, chat.sends],
			() => {
				if (stick) node.scrollTop = node.scrollHeight;
			}
		);
		return () => node.removeEventListener('scroll', onScroll);
	};
}
