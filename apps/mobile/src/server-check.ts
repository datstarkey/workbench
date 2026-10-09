/** An asleep or off-Tailscale machine never answers; give up instead of hanging for minutes. */
export const CONNECT_TIMEOUT_MS = 8000;

/** What `/health` says the server speaks; an older host answers plain `ok`. */
export interface ServerHealth {
	workspaceApi?: number;
}

/** Throws a readable error unless `base` is a Workbench server that accepts `token`. */
export async function verifyServer(base: string, token: string): Promise<ServerHealth> {
	try {
		const res = await fetch(`${base}/health`, { signal: AbortSignal.timeout(CONNECT_TIMEOUT_MS) });
		if (!res.ok) throw new Error(`health check returned ${res.status}`);
		const health = (await res.json().catch(() => null)) as ServerHealth | null;
		// /health is unauthenticated, so check the token on a protected route.
		const authed = await fetch(`${base}/remote/terminals`, {
			headers: { authorization: `Bearer ${token}` },
			signal: AbortSignal.timeout(CONNECT_TIMEOUT_MS)
		});
		if (authed.status === 401) throw new Error('invalid token');
		if (!authed.ok) throw new Error(`server returned ${authed.status}`);
		return typeof health === 'object' && health ? health : {};
	} catch (e) {
		if ((e as Error)?.name === 'TimeoutError') throw new Error('not responding (timed out)');
		throw e;
	}
}
