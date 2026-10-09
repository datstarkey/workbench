// Release builds in CI (CI + SENTRY_AUTH_TOKEN) build hidden source maps, upload them to
// the self-hosted Sentry under the release Sentry.init reports, then delete them so they
// never ship. Anywhere else, including a local shell that happens to hold the token,
// this returns null and the build is unchanged.
export function sentryUploadOptions(release: string) {
	const authToken = process.env.SENTRY_AUTH_TOKEN;
	if (!process.env.CI || !authToken) return null;
	return {
		url: 'https://sentry.starkeydigital.com',
		org: 'starkey-digital',
		project: 'workbench',
		authToken,
		release: { name: release, setCommits: false as const },
		sourcemaps: { filesToDeleteAfterUpload: ['./dist/**/*.map'] },
		telemetry: false
	};
}
