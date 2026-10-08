/** Budget for a request that should answer at once; routes that wait server-side pass more. */
export const DEFAULT_TIMEOUT_MS = 10_000;
/** Routes that read a lot or wait on a process, e.g. a review diff or stopping a session. */
export const SLOW_TIMEOUT_MS = 30_000;

/**
 * Run a request (fetch and body read) with an abort signal that fires after
 * `timeoutMs`, rejecting with a readable error when it does. Without a
 * deadline, requests to a stalled server pile up in the WebView's per-host
 * connection pool and all fire at once when it recovers. `what` names the
 * request, e.g. `GET /agent`. A timer, not `AbortSignal.timeout`: macOS 10.15's
 * WebKit lacks it, and older WebKit rejects with a plain `AbortError`.
 */
export async function withTimeout<T>(
	what: string,
	timeoutMs: number,
	run: (signal: AbortSignal) => Promise<T>
): Promise<T> {
	const ctrl = new AbortController();
	let timedOut = false;
	const timer = setTimeout(() => {
		timedOut = true;
		ctrl.abort();
	}, timeoutMs);
	try {
		return await run(ctrl.signal);
	} catch (e) {
		if (!timedOut) throw e;
		throw new Error(
			`${what} timed out after ${Math.round(timeoutMs / 1000)}s: the server is not responding`
		);
	} finally {
		clearTimeout(timer);
	}
}
