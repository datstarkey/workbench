import { svelte } from '@sveltejs/vite-plugin-svelte';
import { svelteTesting } from '@testing-library/svelte/vite';
import { defineConfig } from 'vitest/config';
import { resolve } from 'path';

export default defineConfig({
	plugins: [svelte(), svelteTesting()],
	resolve: {
		alias: {
			$lib: resolve('./src/lib'),
			$components: resolve('./src/lib/components'),
			$features: resolve('./src/lib/features'),
			$stores: resolve('./src/lib/stores'),
			$types: resolve('./src/types')
		}
	},
	test: {
		projects: [
			{
				extends: true,
				test: {
					name: 'unit',
					include: ['src/**/*.test.ts'],
					environment: 'node'
				}
			},
			{
				extends: true,
				test: {
					name: 'component',
					include: ['src/**/*.test.svelte.ts'],
					environment: 'jsdom',
					setupFiles: ['src/test/setup.ts']
				}
			}
		]
	}
});
