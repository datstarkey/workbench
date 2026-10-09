import { openUrl } from '@tauri-apps/plugin-opener';

/** Uses the system URL handler without the opener plugin's inAppBrowser mode. */
export function openExternal(url: string): void {
	openUrl(url).catch((e) => console.warn('[mobile] open url', url, e));
}
