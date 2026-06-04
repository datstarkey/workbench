import { OverlayScrollbars, type PartialOptions } from 'overlayscrollbars';
import 'overlayscrollbars/overlayscrollbars.css';
import type { Attachment } from 'svelte/attachments';

/**
 * Svelte attachment that turns an element into an OverlayScrollbars container:
 * floating, auto-hiding scrollbars that don't take up layout space, replacing
 * the chunky native scrollbar. Styled via the `os-theme-wb` theme (see app.css).
 *
 * Note: OverlayScrollbars restructures the element's children into a generated
 * viewport. For layout-sensitive containers (e.g. flex strips) put the layout
 * on a single inner wrapper, not on the element this is attached to.
 *
 * @param options Per-call overrides, e.g. `{ overflow: { y: 'hidden' } }` for a
 *   horizontal-only strip. Merged shallowly over the defaults below.
 */
export function overlayScrollbars(options: PartialOptions = {}): Attachment<HTMLElement> {
	return (element) => {
		const instance = OverlayScrollbars(element, {
			scrollbars: {
				theme: 'os-theme-wb',
				autoHide: 'leave',
				autoHideDelay: 300
			},
			...options
		});
		return () => instance.destroy();
	};
}
