import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';
import { sentryVitePlugin } from '@sentry/vite-plugin';
import { defineConfig } from 'vite';
import { resolve } from 'path';
import tauriConf from './src-tauri/tauri.conf.json';
import { sentryUploadOptions } from '../../scripts/sentry-vite';

const host = process.env.TAURI_DEV_HOST;

const sentry = sentryUploadOptions(`workbench@${tauriConf.version}`);

export default defineConfig({
	define: {
		__APP_VERSION__: JSON.stringify(tauriConf.version)
	},
	plugins: [tailwindcss(), svelte(), sentry ? sentryVitePlugin(sentry) : []],
	resolve: {
		alias: {
			$lib: resolve('./src/lib'),
			$components: resolve('./src/lib/components'),
			$features: resolve('./src/lib/features'),
			$stores: resolve('./src/lib/stores'),
			$types: resolve('../../packages/types/src')
		}
	},
	clearScreen: false,
	server: {
		port: 1420,
		strictPort: true,
		host: host || false,
		hmr: host
			? {
					protocol: 'ws',
					host,
					port: 1421
				}
			: undefined
	},
	build: {
		outDir: 'dist',
		sourcemap: sentry ? 'hidden' : false
	}
});
