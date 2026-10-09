/**
 * terminal.ts — unit tests.
 *
 * The old terminal:data / terminal:exit Tauri event-routing tests have been
 * removed: the singleton fan-out registry (`onSessionTerminalData`,
 * `onTerminalData`) is only used by the IPC xterm path which is being retired
 * in favour of direct WebSocket connections (see TerminalConnection).
 *
 * What remains testable here:
 *   - Native terminal IPC wrappers (attachNativeTerminal, resizeNativeTerminal,
 *     etc.) — thin `invoke` shims whose correctness is worth a smoke test.
 *   - Integration-status helpers (checkCodexIntegration, applyCodexIntegration)
 *     which are unrelated to the xterm path.
 *
 * xterm WS contract tests live in:
 *   apps/desktop/src/lib/features/terminal/terminal-connection.test.ts
 */

import { describe, expect, it, vi, beforeEach } from 'vitest';
import { invokeSpy, clearInvokeMocks } from '../../test/tauri-mocks';

beforeEach(() => {
	clearInvokeMocks();
});

describe('native terminal IPC wrappers', () => {
	it("attachNativeTerminal shows the pane's server terminal", async () => {
		invokeSpy.mockResolvedValueOnce(undefined);
		const { attachNativeTerminal } = await import('./terminal');
		const request = {
			terminalId: 'term-1',
			x: 0,
			y: 0,
			width: 800,
			height: 600,
			fontSize: 13
		};

		await attachNativeTerminal(request);

		expect(invokeSpy).toHaveBeenCalledWith('attach_native_terminal', request);
	});

	it('resizeNativeTerminal invokes resize_native_terminal', async () => {
		invokeSpy.mockResolvedValueOnce(undefined);
		const { resizeNativeTerminal } = await import('./terminal');

		await resizeNativeTerminal('ses-1', 10, 20, 800, 600);

		expect(invokeSpy).toHaveBeenCalledWith('resize_native_terminal', {
			sessionId: 'ses-1',
			x: 10,
			y: 20,
			width: 800,
			height: 600
		});
	});

	it('detachNativeTerminal invokes detach_native_terminal', async () => {
		invokeSpy.mockResolvedValueOnce(undefined);
		const { detachNativeTerminal } = await import('./terminal');

		await detachNativeTerminal('ses-1');

		expect(invokeSpy).toHaveBeenCalledWith('detach_native_terminal', { sessionId: 'ses-1' });
	});

	it('setNativeTerminalVisible invokes set_native_terminal_visible', async () => {
		invokeSpy.mockResolvedValueOnce(undefined);
		const { setNativeTerminalVisible } = await import('./terminal');

		await setNativeTerminalVisible('ses-1', false);

		expect(invokeSpy).toHaveBeenCalledWith('set_native_terminal_visible', {
			sessionId: 'ses-1',
			visible: false
		});
	});

	it('writeNativeTerminal invokes write_native_terminal', async () => {
		invokeSpy.mockResolvedValueOnce(undefined);
		const { writeNativeTerminal } = await import('./terminal');

		await writeNativeTerminal('ses-1', 'ls\n');

		expect(invokeSpy).toHaveBeenCalledWith('write_native_terminal', {
			sessionId: 'ses-1',
			data: 'ls\n'
		});
	});

	it('isNativeTerminalAvailable invokes is_native_terminal_available', async () => {
		invokeSpy.mockResolvedValueOnce(true);
		const { isNativeTerminalAvailable } = await import('./terminal');

		const result = await isNativeTerminalAvailable();

		expect(invokeSpy).toHaveBeenCalledWith('is_native_terminal_available');
		expect(result).toBe(true);
	});
});

describe('integration-status helpers', () => {
	it('checkCodexIntegration invokes check_codex_integration', async () => {
		invokeSpy.mockResolvedValueOnce({ needsChanges: true, description: 'missing' });
		const { checkCodexIntegration } = await import('./terminal');

		const result = await checkCodexIntegration();

		expect(invokeSpy).toHaveBeenCalledWith('check_codex_integration');
		expect(result).toEqual({ needsChanges: true, description: 'missing' });
	});

	it('applyCodexIntegration invokes apply_codex_integration', async () => {
		invokeSpy.mockResolvedValueOnce(false);
		const { applyCodexIntegration } = await import('./terminal');

		const result = await applyCodexIntegration();

		expect(invokeSpy).toHaveBeenCalledWith('apply_codex_integration');
		expect(result).toBe(false);
	});
});

// Suppress the unused-import lint warning: vi is used in beforeEach/it hooks
void vi;
