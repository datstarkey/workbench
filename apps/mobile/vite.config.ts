import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';
import { defineConfig } from 'vite';

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

export default defineConfig({
	plugins: [tailwindcss(), svelte(), stripCrossorigin()],
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
		target: 'es2021'
	}
});
