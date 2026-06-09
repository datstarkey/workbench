/**
 * terminal.ts now exports:
 *  - Integration checks (checkClaudeIntegration etc.)
 *  - Native SwiftTerm IPC wrappers (createNativeTerminal etc.)
 *  - onSessionTerminalExit — kept for NativeTerminalPane (PtyManager IPC path)
 *
 * The xterm IPC layer (createTerminal / writeTerminal / resizeTerminal /
 * killTerminal / onTerminalData / onSessionTerminalData / onTerminalExit /
 * cleanupSessionInput / sessionWriteChains) has been REMOVED. xterm panes now
 * use TerminalConnection (loopback WebSocket to the embedded server).
 */
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { clearListeners, listenSpy } from '../../test/tauri-mocks';

describe('terminal.ts — contract verification', () => {
	beforeEach(() => {
		vi.resetModules();
		clearListeners();
	});

	it('exports integration checks and native terminal wrappers', async () => {
		const mod = await import('./terminal');
		expect(typeof mod.checkClaudeIntegration).toBe('function');
		expect(typeof mod.checkCodexIntegration).toBe('function');
		expect(typeof mod.applyClaudeIntegration).toBe('function');
		expect(typeof mod.applyCodexIntegration).toBe('function');
		expect(typeof mod.createNativeTerminal).toBe('function');
		expect(typeof mod.resizeNativeTerminal).toBe('function');
		expect(typeof mod.setNativeTerminalVisible).toBe('function');
		expect(typeof mod.killNativeTerminal).toBe('function');
		expect(typeof mod.writeNativeTerminal).toBe('function');
		expect(typeof mod.isNativeTerminalAvailable).toBe('function');
	});

	it('still exports onSessionTerminalExit for NativeTerminalPane (PtyManager path)', async () => {
		const mod = await import('./terminal');
		expect(typeof mod.onSessionTerminalExit).toBe('function');
	});

	it('registers a single terminal:exit listener regardless of session subscribers', async () => {
		const { onSessionTerminalExit } = await import('./terminal');
		await onSessionTerminalExit('native-a', vi.fn());
		await onSessionTerminalExit('native-b', vi.fn());
		const exitListenCalls = listenSpy.mock.calls.filter((call) => call[0] === 'terminal:exit');
		expect(exitListenCalls).toHaveLength(1);
	});

	it('does NOT export the removed xterm IPC functions', async () => {
		const mod = await import('./terminal');
		expect((mod as Record<string, unknown>)['createTerminal']).toBeUndefined();
		expect((mod as Record<string, unknown>)['writeTerminal']).toBeUndefined();
		expect((mod as Record<string, unknown>)['resizeTerminal']).toBeUndefined();
		expect((mod as Record<string, unknown>)['killTerminal']).toBeUndefined();
		expect((mod as Record<string, unknown>)['onTerminalData']).toBeUndefined();
		expect((mod as Record<string, unknown>)['onSessionTerminalData']).toBeUndefined();
		expect((mod as Record<string, unknown>)['onTerminalExit']).toBeUndefined();
		expect((mod as Record<string, unknown>)['cleanupSessionInput']).toBeUndefined();
	});
});
