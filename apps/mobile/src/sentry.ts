import * as Sentry from '@sentry/svelte';

// The desktop's public ingest DSN (write-only key); releases tell the apps apart.
// Overridable via VITE_SENTRY_DSN at build time.
const DSN =
	import.meta.env.VITE_SENTRY_DSN ??
	'https://708b68ab317b3d17816c9f8135337d11@sentry.starkeydigital.com/20';

/** Error tracking for release builds; a no-op in dev, tests, or without a DSN. */
export function initSentry(): void {
	if (import.meta.env.MODE === 'test' || !import.meta.env.PROD || !DSN) return;
	Sentry.init({
		dsn: DSN,
		release: `workbench-mobile@${__APP_VERSION__}`,
		environment: 'production',
		tracesSampleRate: 0
	});
}
