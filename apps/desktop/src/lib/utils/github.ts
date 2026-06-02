import { invoke } from '@tauri-apps/api/core';

export async function openInGitHub(url: string): Promise<void> {
	try {
		await invoke('open_url', { url });
	} catch (e) {
		console.error('[github] Failed to open URL:', e);
	}
}

export function branchUrl(repoUrl: string, branch: string): string {
	return `${repoUrl}/tree/${encodeURIComponent(branch)}`;
}
