import { check } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';

export async function checkForAppUpdate() {
	const update = await check();
	if (!update) return null;
	return {
		version: update.version,
		body: update.body,
		async install(onProgress?: (percent: number) => void) {
			let totalLength = 0;
			let downloaded = 0;
			await update.downloadAndInstall((event) => {
				if (event.event === 'Started' && event.data.contentLength) {
					totalLength = event.data.contentLength;
				} else if (event.event === 'Progress') {
					downloaded += event.data.chunkLength;
					if (totalLength > 0) onProgress?.(downloaded / totalLength);
				}
			});
			await relaunch();
		}
	};
}
