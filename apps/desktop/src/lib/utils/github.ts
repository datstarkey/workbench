export function branchUrl(repoUrl: string, branch: string): string {
	return `${repoUrl}/tree/${encodeURIComponent(branch)}`;
}
