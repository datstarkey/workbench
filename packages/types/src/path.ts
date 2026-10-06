/** The last segment of a Unix or Windows path. */
export function baseName(path: string): string {
	const segments = path.replace(/\\/g, '/').split('/').filter(Boolean);
	return segments[segments.length - 1] || path;
}
