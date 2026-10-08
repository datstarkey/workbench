/** Budget for a request that should answer at once; routes that wait server-side pass more. */
export const DEFAULT_TIMEOUT_MS = 10_000;

/**
 * Run a request (fetch and body read) under `AbortSignal.timeout`, rejecting
 * with a readable error when it runs out. Without a deadline, requests to a
 * stalled server pile up in the WebView's per-host connection pool and all
 * fire at once when it recovers. `what` names the request, e.g. `GET /agent`.
 */
export async function withTimeout<T>(
	what: string,
	timeoutMs: number,
	run: (signal: AbortSignal) => Promise<T>
): Promise<T> {
	try {
		return await run(AbortSignal.timeout(timeoutMs));
	} catch (e) {
		if ((e as { name?: string } | null)?.name !== 'TimeoutError') throw e;
		throw new Error(
			`${what} timed out after ${Math.round(timeoutMs / 1000)}s: the server is not responding`
		);
	}
}
