import './app.css';
import App from './App.svelte';
import { mount } from 'svelte';

// Force dark mode
document.documentElement.classList.add('dark');

const app = mount(App, {
	target: document.getElementById('app')!
});

export default app;
