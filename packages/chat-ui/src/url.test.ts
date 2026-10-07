import { expect, it } from 'vitest';
import { safeExternalUrl } from './url';

it('only opens http(s) links', () => {
	expect(safeExternalUrl('javascript:alert(1)')).toBeNull();
	expect(safeExternalUrl('file:///etc/passwd')).toBeNull();
	expect(safeExternalUrl('not a url')).toBeNull();
	expect(safeExternalUrl(undefined)).toBeNull();
	expect(safeExternalUrl('https://example.com/sign-in')).toBe('https://example.com/sign-in');
});
