import { invoke } from '@tauri-apps/api/core';

/** Open a project folder in VS Code */
export async function openInVSCode(projectPath: string): Promise<void> {
	try {
		await invoke('open_in_vscode', { path: projectPath });
	} catch (e) {
		console.error('[vscode] Failed to open VS Code:', e);
	}
}
