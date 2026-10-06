export { baseName } from '$types/path';

/** Return the effective working directory for a workspace (worktree or project path) */
export function effectivePath(ws: { projectPath: string; worktreePath?: string }): string {
	return ws.worktreePath ?? ws.projectPath;
}
