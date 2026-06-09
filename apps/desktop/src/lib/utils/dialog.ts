import { open } from '@tauri-apps/plugin-dialog';

export async function selectFolder(
	defaultPath?: string,
	title = 'Select Project Folder'
): Promise<string | null> {
	const result = await open({
		directory: true,
		multiple: false,
		defaultPath,
		title
	});
	return result as string | null;
}
