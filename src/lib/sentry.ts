import * as Sentry from '@sentry/svelte';

// Public ingest DSN (write-only key). Overridable via VITE_SENTRY_DSN at build time.
const DSN =
	import.meta.env.VITE_SENTRY_DSN ??
	'https://708b68ab317b3d17816c9f8135337d11@sentry.starkeydigital.com/20';

/**
 * Initialise Sentry error tracking.
 *
 * Only runs in production builds (`tauri build`). During `tauri dev`
 * (`import.meta.env.DEV`) this is a no-op, so local development never
 * reports errors upstream.
 */
export function initSentry(): void {
	if (!import.meta.env.PROD) return;
	if (!DSN) return;

	Sentry.init({
		dsn: DSN,
		release: `workbench@${__APP_VERSION__}`,
		environment: 'production',
		// Errors only — no performance/session tracing for a desktop app.
		tracesSampleRate: 0
	});
}
