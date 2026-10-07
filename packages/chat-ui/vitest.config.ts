import { svelte } from '@sveltejs/vite-plugin-svelte';
import { defineConfig } from 'vitest/config';

export default defineConfig({
	plugins: [svelte()],
	// Svelte's client runtime, so `$effect`s run under `flushSync`.
	resolve: { conditions: ['browser'] },
	test: {
		environment: 'node',
		include: ['src/**/*.test.ts', 'src/**/*.test.svelte.ts']
	}
});
