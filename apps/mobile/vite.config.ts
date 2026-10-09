import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';
import { sentryVitePlugin } from '@sentry/vite-plugin';
import { defineConfig } from 'vite';
import tauriConf from './src-tauri/tauri.conf.json';

// Tauri mobile injects the dev host (device-reachable LAN/tailscale address).
// For plain web testing we bind 0.0.0.0 so a phone on the same tailnet/LAN can load it.
const host = process.env.TAURI_DEV_HOST;

// Tauri's Android webview serves assets from a custom scheme; the `crossorigin`
// attribute Vite adds to module/style tags can make those loads fail silently
// (black screen). Strip it from the built index.html.
function stripCrossorigin() {
	return {
		name: 'strip-crossorigin',
		transformIndexHtml(html: string) {
			return html.replace(/ crossorigin/g, '');
		}
	};
}

// Release builds in CI set SENTRY_AUTH_TOKEN: source maps are built hidden, uploaded
// to the self-hosted Sentry under the release Sentry.init reports, then deleted so
// they never ship. Without the token (local builds, PRs) nothing changes.
const sentryToken = process.env.SENTRY_AUTH_TOKEN;
const sentry = sentryToken
	? sentryVitePlugin({
			url: 'https://sentry.starkeydigital.com',
			org: 'starkey-digital',
			project: 'workbench',
			authToken: sentryToken,
			release: { name: `workbench-mobile@${tauriConf.version}`, setCommits: false },
			sourcemaps: { filesToDeleteAfterUpload: ['./dist/**/*.map'] },
			telemetry: false
		})
	: [];

export default defineConfig({
	define: {
		__APP_VERSION__: JSON.stringify(tauriConf.version)
	},
	plugins: [tailwindcss(), svelte(), stripCrossorigin(), sentry],
	clearScreen: false,
	server: {
		host: host || '0.0.0.0',
		port: 1430,
		strictPort: true,
		hmr: host
			? {
					protocol: 'ws',
					host,
					port: 1431
				}
			: undefined
	},
	build: {
		outDir: 'dist',
		target: 'es2021',
		sourcemap: sentryToken ? 'hidden' : false
	}
});
