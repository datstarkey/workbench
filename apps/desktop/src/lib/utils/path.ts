/** Extract the last segment from a file path */
export function baseName(path: string): string {
	const segments = path.replace(/\\/g, '/').split('/').filter(Boolean);
	return segments[segments.length - 1] || path;
}

/** Return the effective working directory for a workspace (worktree or project path) */
export function effectivePath(ws: { projectPath: string; worktreePath?: string }): string {
	return ws.worktreePath ?? ws.projectPath;
}
