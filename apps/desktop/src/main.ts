import './app.css';
import { mount } from 'svelte';
import { initSentry } from '$lib/sentry';

// Error tracking — no-op in dev, reports only in production builds.
initSentry();

// Force dark mode
document.documentElement.classList.add('dark');

const target = document.getElementById('app')!;

// `index.html?view=settings` is loaded by the dedicated settings OS window;
// every other window mounts the main app shell.
const view = new URLSearchParams(window.location.search).get('view');

const app =
	view === 'settings'
		? mount((await import('./SettingsWindow.svelte')).default, { target })
		: mount((await import('./App.svelte')).default, { target });

export default app;
