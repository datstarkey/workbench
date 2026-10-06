import { invoke, isTauri } from '@tauri-apps/api/core';

export const supportsDictation = () => isTauri() && /Android/i.test(navigator.userAgent);

/** Android's recognizer owns the microphone and returns editable text, never a prompt. */
export async function dictate(): Promise<string | null> {
	const result = await invoke<{ text: string | null }>('plugin:speech-input|recognize');
	return result.text;
}
