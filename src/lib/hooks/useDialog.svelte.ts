import { open } from '@tauri-apps/plugin-dialog';

export async function selectFolder(defaultPath?: string): Promise<string | null> {
	const result = await open({
		directory: true,
		multiple: false,
		defaultPath,
		title: 'Select Project Folder'
	});
	return result as string | null;
}
