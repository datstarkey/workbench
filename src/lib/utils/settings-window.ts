import { WebviewWindow } from '@tauri-apps/api/webviewWindow';
import type { SettingsWindowBounds } from '$types/workbench';

/** Label of the dedicated settings OS window. */
export const SETTINGS_WINDOW_LABEL = 'settings';

const DEFAULT_WIDTH = 840;
const DEFAULT_HEIGHT = 560;
const MIN_WIDTH = 560;
const MIN_HEIGHT = 380;

/**
 * Open (or focus) the settings window as a real, separate OS window.
 *
 * The window reuses `index.html` with a `?view=settings` query param; `main.ts`
 * mounts the settings-only root for that view. The active project path is passed
 * through the query string so Claude project-scope settings resolve correctly.
 * Geometry is restored from the persisted `settingsWindowBounds`.
 */
export async function openSettingsWindow(
	projectPath: string | null,
	bounds: SettingsWindowBounds | null
): Promise<void> {
	const existing = await WebviewWindow.getByLabel(SETTINGS_WINDOW_LABEL);
	if (existing) {
		await existing.unminimize().catch(() => {});
		await existing.setFocus();
		return;
	}

	const params = new URLSearchParams({ view: 'settings' });
	if (projectPath) params.set('project', projectPath);

	const win = new WebviewWindow(SETTINGS_WINDOW_LABEL, {
		url: `index.html?${params.toString()}`,
		title: 'Settings',
		width: bounds?.width ?? DEFAULT_WIDTH,
		height: bounds?.height ?? DEFAULT_HEIGHT,
		x: bounds?.x,
		y: bounds?.y,
		minWidth: MIN_WIDTH,
		minHeight: MIN_HEIGHT,
		resizable: true,
		decorations: true,
		focus: true,
		center: bounds == null
	});

	win.once('tauri://error', (event) => {
		console.error('Failed to open settings window', event.payload);
	});
}
