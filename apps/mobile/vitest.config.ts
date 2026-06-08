import { svelte } from '@sveltejs/vite-plugin-svelte';
import { defineConfig } from 'vitest/config';

// Unit tests for the mobile app's logic (MobileClient store + pure helpers). The
// Svelte components themselves (xterm-backed Terminal) aren't rendered here — the
// testable logic lives in client.svelte.ts / terminal-url.ts.
export default defineConfig({
	plugins: [svelte()],
	test: {
		environment: 'jsdom',
		include: ['src/**/*.test.ts', 'src/**/*.test.svelte.ts']
	}
});
