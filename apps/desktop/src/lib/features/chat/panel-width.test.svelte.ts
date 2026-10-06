import { describe, expect, it } from 'vitest';
import { PanelWidth, TASKS_PANEL_WIDTH } from './panel-width.svelte';

function memory(initial?: string) {
	const data: Record<string, string> = initial ? { 'workbench.chat.tasksPanelWidth': initial } : {};
	return {
		data,
		getItem: (k: string) => data[k] ?? null,
		setItem: (k: string, v: string) => void (data[k] = v)
	};
}

describe('PanelWidth', () => {
	it('starts at the default, or the saved width clamped to range', () => {
		expect(new PanelWidth(memory()).width).toBe(TASKS_PANEL_WIDTH.initial);
		expect(new PanelWidth(memory('400')).width).toBe(400);
		expect(new PanelWidth(memory('5000')).width).toBe(TASKS_PANEL_WIDTH.max);
		expect(new PanelWidth(memory('junk')).width).toBe(TASKS_PANEL_WIDTH.initial);
		expect(new PanelWidth(null).width).toBe(TASKS_PANEL_WIDTH.initial);
	});

	it('widens as the left edge is dragged left, and saves on release', () => {
		const storage = memory();
		const panel = new PanelWidth(storage);
		panel.dragTo(100); // no drag in progress
		expect(panel.width).toBe(288);
		panel.beginDrag(800);
		panel.dragTo(700);
		expect(panel.width).toBe(388);
		panel.dragTo(1500);
		expect(panel.width).toBe(TASKS_PANEL_WIDTH.min);
		expect(storage.data).toEqual({});
		panel.endDrag();
		expect(storage.data).toEqual({ 'workbench.chat.tasksPanelWidth': '240' });
	});

	it('steps with arrow keys, jumps with Home/End, ignores other keys', () => {
		const panel = new PanelWidth(memory());
		expect(panel.key('ArrowLeft')).toBe(true);
		expect(panel.width).toBe(304);
		panel.key('ArrowRight', true);
		expect(panel.width).toBe(240);
		panel.key('End');
		expect(panel.width).toBe(TASKS_PANEL_WIDTH.max);
		panel.key('Home');
		expect(panel.width).toBe(TASKS_PANEL_WIDTH.min);
		expect(panel.key('Enter')).toBe(false);
	});

	it('survives storage that throws', () => {
		const broken = {
			getItem: () => {
				throw new Error('blocked');
			},
			setItem: () => {
				throw new Error('full');
			}
		};
		const panel = new PanelWidth(broken);
		expect(panel.width).toBe(TASKS_PANEL_WIDTH.initial);
		expect(panel.key('ArrowLeft')).toBe(true);
	});
});
