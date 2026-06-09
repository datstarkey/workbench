/// <reference types="svelte" />
/// <reference types="vite/client" />

// Injected by Vite `define` from src-tauri/tauri.conf.json `version`.
declare const __APP_VERSION__: string;

interface ImportMetaEnv {
	readonly VITE_SENTRY_DSN?: string;
}

interface ImportMeta {
	readonly env: ImportMetaEnv;
}
