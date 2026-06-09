import './app.css';
import { mount } from 'svelte';
import App from './App.svelte';

// Dark mode forced on, same as desktop.
document.documentElement.classList.add('dark');

async function boot() {
	// Forward console.* to the Rust log plugin (→ logcat on Android). Best-effort:
	// never let logging setup block the app from mounting.
	try {
		const { attachConsole } = await import('@tauri-apps/plugin-log');
		await attachConsole();
	} catch {
		/* logging optional */
	}

	mount(App, { target: document.getElementById('app')! });
}

boot();
