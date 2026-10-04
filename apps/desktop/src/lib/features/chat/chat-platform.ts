import { invoke } from '@tauri-apps/api/core';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import { open } from '@tauri-apps/plugin-shell';
import type { ChatPlatform } from '@workbench/chat-ui';
import type { ChatAttachment } from '$types/workbench';

export const desktopChatPlatform: ChatPlatform = {
	openLink(href) {
		open(href).catch((error) => console.warn('[chat] could not open link', error));
	},

	/** Tauri takes OS file drops before the page sees them; keep the ones over `node`. */
	watchDrops(node, on) {
		const inside = (x: number, y: number) => {
			const r = node.getBoundingClientRect();
			const px = x / window.devicePixelRatio;
			const py = y / window.devicePixelRatio;
			return px >= r.left && px <= r.right && py >= r.top && py <= r.bottom;
		};
		let stop: (() => void) | null = null;
		let disposed = false;
		void getCurrentWebview()
			.onDragDropEvent(async (event) => {
				const p = event.payload;
				if (p.type === 'leave') return on.hover(false);
				const over = inside(p.position.x, p.position.y);
				if (p.type !== 'drop') return on.hover(over);
				on.hover(false);
				if (!over) return;
				const results = await Promise.allSettled(
					p.paths.map((path) => invoke<ChatAttachment>('read_chat_attachment', { path }))
				);
				const failed = results.find((r): r is PromiseRejectedResult => r.status === 'rejected');
				on.drop(
					results.flatMap((r) => (r.status === 'fulfilled' ? [r.value] : [])),
					failed ? String(failed.reason) : null
				);
			})
			.then((unlisten) => {
				if (disposed) unlisten();
				else stop = unlisten;
			})
			.catch(() => {});
		return () => {
			disposed = true;
			stop?.();
		};
	}
};
