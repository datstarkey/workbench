import { describe, expect, it, vi } from 'vitest';
import { TabReorder } from './tab-reorder.svelte';

const drag = () => ({ preventDefault: vi.fn(), dataTransfer: null }) as unknown as DragEvent;

describe('TabReorder', () => {
	function setup() {
		const onMove = vi.fn();
		const reorder = new TabReorder(() => ['a', 'b', 'c'], onMove);
		return { reorder, onMove };
	}

	it('shows the drop side relative to the dragged tab', () => {
		const { reorder } = setup();
		reorder.handlers('b').ondragstart(drag());

		reorder.handlers('c').ondragover(drag());
		expect(reorder.dropSide('c')).toBe('after');

		reorder.handlers('a').ondragover(drag());
		expect(reorder.dropSide('a')).toBe('before');
		expect(reorder.dropSide('c')).toBeNull();
		expect(reorder.dropSide('b')).toBeNull();
	});

	it('moves on drop and resets', () => {
		const { reorder, onMove } = setup();
		reorder.handlers('a').ondragstart(drag());

		reorder.handlers('c').ondrop(drag());

		expect(onMove).toHaveBeenCalledWith('a', 'c');
		expect(reorder.dragging).toBeNull();
		expect(reorder.over).toBeNull();
	});

	it('ignores drags that started in another strip', () => {
		const { reorder, onMove } = setup();
		const over = drag();

		reorder.handlers('a').ondragover(over);
		reorder.handlers('a').ondrop(drag());

		expect(over.preventDefault).not.toHaveBeenCalled();
		expect(onMove).not.toHaveBeenCalled();
	});
});
