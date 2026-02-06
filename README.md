# Workbench

Workbench is an Electron + SvelteKit terminal-focused project manager.

## Features in this version

- Sidebar project manager with add/edit/remove
- Projects persisted to `~/.workbench/projects.json`
- Multi-project tabbed workspace (drag to reorder)
- Multiple terminal tabs per project
- Horizontal/vertical terminal splitting
- Real shell terminals via `node-pty` + `xterm.js`
- Startup command support (runs in first terminal pane)
- `Open in VS Code` project action (`code <path>`)
- Terminal/session lifecycle cleanup on tab close and app quit

## Stack

- Electron (main + preload)
- SvelteKit (renderer)
- TypeScript
- xterm.js + node-pty

## Project config

Config file path: `~/.workbench/projects.json`

Example:

```json
{
	"projects": [
		{ "name": "Client Site", "path": "/code/client-site", "startupCommand": "pnpm dev" },
		{ "name": "API", "path": "/code/api", "shell": "/bin/zsh" }
	]
}
```

## Run

```bash
bun install
bun run dev
```

Renderer dev server runs on `http://127.0.0.1:4173`, and Electron opens the desktop window automatically.

## Build

````bash
bun run build

## Typecheck

```bash
bun run check
````

```

## Notes

- Ensure VS Code CLI is installed: Command Palette -> `Shell Command: Install 'code' command in PATH`
- `node-pty` may require native build tooling depending on your environment.
```
