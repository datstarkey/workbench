import { beforeEach, describe, expect, it, vi } from 'vitest';
import { clearListeners, emitMockEvent, listenSpy } from '../../test/tauri-mocks';

describe('terminal event routing', () => {
	beforeEach(() => {
		vi.resetModules();
		clearListeners();
	});

	it('routes session data to matching listeners only', async () => {
		const { onSessionTerminalData } = await import('./terminal');
		const sessionA = vi.fn();
		const sessionB = vi.fn();

		const unlistenA = await onSessionTerminalData('pane-a', sessionA);
		await onSessionTerminalData('pane-b', sessionB);

		emitMockEvent('terminal:data', { sessionId: 'pane-a', data: 'hello' });
		expect(sessionA).toHaveBeenCalledWith({ sessionId: 'pane-a', data: 'hello' });
		expect(sessionB).not.toHaveBeenCalled();

		unlistenA();
		emitMockEvent('terminal:data', { sessionId: 'pane-a', data: 'again' });
		expect(sessionA).toHaveBeenCalledTimes(1);
	});

	it('registers one tauri data listener regardless of session subscribers', async () => {
		const { onSessionTerminalData } = await import('./terminal');
		await onSessionTerminalData('pane-a', vi.fn());
		await onSessionTerminalData('pane-b', vi.fn());

		const dataListenCalls = listenSpy.mock.calls.filter((call) => call[0] === 'terminal:data');
		expect(dataListenCalls).toHaveLength(1);
	});

	it('still supports global terminal data listeners', async () => {
		const { onTerminalData } = await import('./terminal');
		const globalCb = vi.fn();
		await onTerminalData(globalCb);

		emitMockEvent('terminal:data', { sessionId: 'pane-a', data: 'payload' });
		expect(globalCb).toHaveBeenCalledWith({ sessionId: 'pane-a', data: 'payload' });
	});
});
