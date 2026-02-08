# CLAUDE.md

## Project Overview

Workbench is a Tauri v2 + Svelte 5 desktop terminal manager. Users add local project folders, and each project gets a tabbed workspace with real shell terminals (via `portable-pty` + `xterm.js`). Config is persisted to `~/.workbench/projects.json`, workspaces to `~/.workbench/workspaces.json`.

## Commands

- **Install:** `bun install`
- **Dev:** `bun run dev` (runs `tauri dev` — starts Vite on port 1420 + Rust backend)
- **Build:** `bun run build` (runs `tauri build` — produces .app and .dmg)
- **Typecheck:** `bun run check` (svelte-check)
- **Lint:** `bun run lint` (prettier --check + eslint)
- **Format:** `bun run format` (prettier --write)

Use **Bun** exclusively — do not introduce npm/yarn/pnpm lockfiles.

## Architecture

### Tauri v2 model

- **Rust backend** (`src-tauri/src/`): Manages windows, PTY sessions, project config I/O, and Tauri commands via Cargo.
- **Frontend** (`src/`): Plain Svelte 5 + Vite app (no SvelteKit). In dev, Tauri loads `http://localhost:1420`; in production, loads `dist/index.html`.

### Rust source structure (`src-tauri/src/`)

- `main.rs` — Entry point, calls `workbench_lib::run()`
- `lib.rs` — Registers plugins (dialog, store, shell), manages `PtyManager` state, registers all commands
- `types.rs` — Shared Rust types: `ProjectConfig`, `CreateTerminalRequest`, `TerminalDataEvent`, workspace snapshots. All `Serialize`/`Deserialize`
- `config.rs` — Project and workspace persistence to `~/.workbench/`, Claude JSONL session discovery from `~/.claude/`
- `pty.rs` — `PtyManager` with PTY session lifecycle: spawn, write, resize, kill. Reader threads (8KB buffer) emit `terminal:data` events via `AppHandle::emit()`
- `commands.rs` — `#[tauri::command]` handlers for all IPC

### Frontend IPC

All IPC uses `invoke()` from `@tauri-apps/api/core` and `listen()` from `@tauri-apps/api/event`. Commands: `list_projects`, `save_projects`, `create_terminal`, `write_terminal`, `resize_terminal`, `kill_terminal`, `open_in_vscode`, `load_workspaces`, `save_workspaces`. Events from Rust→frontend: `terminal:data`, `terminal:exit`.

### Frontend structure

- `src/App.svelte` — Main app shell. Imports stores, mounts components, handles initialization
- `src/main.ts` — Svelte mount point, imports CSS, sets dark mode
- `src/lib/stores/projects.svelte.ts` — Project CRUD store using `$state` + `invoke()`
- `src/lib/stores/workspaces.svelte.ts` — Workspace/tab/pane state store with persistence
- `src/lib/stores/claudeSessions.svelte.ts` — Claude session discovery, polling, quiescence detection
- `src/lib/hooks/useTerminal.svelte.ts` — Wraps Tauri terminal IPC (create, write, resize, kill, listen)
- `src/lib/hooks/useDialog.svelte.ts` — Wraps `@tauri-apps/plugin-dialog` folder picker
- `src/lib/components/ProjectSidebar.svelte` — Sidebar with project list and actions
- `src/lib/components/WorkspaceTabs.svelte` — Top-level project workspace tab bar
- `src/lib/components/TerminalTabs.svelte` — Terminal tab bar with split/new-tab actions
- `src/lib/components/ClaudeSessionMenu.svelte` — Dropdown for resuming past Claude sessions
- `src/lib/components/TerminalGrid.svelte` — Renders terminal panes in flex layout
- `src/lib/components/TerminalPane.svelte` — xterm.js terminal component
- `src/lib/components/ProjectDialog.svelte` — Create/edit project form
- `src/lib/components/ConfirmDialog.svelte` — Confirmation dialog (replaces browser confirm())
- `src/lib/components/EmptyState.svelte` — "No workspace open" placeholder
- `src/lib/components/settings/SettingsSheet.svelte` — Settings panel (slide-out sheet)
- `src/lib/components/ui/` — shadcn-svelte components (badge, button, card, dialog, input, etc.)
- `src/lib/utils/format.ts` — Shared formatting utilities (e.g., `formatSessionDate`)
- `src/lib/utils/path.ts` — Shared path utilities (e.g., `baseName`)
- `src/lib/utils/claude.ts` — Claude CLI command builders (`claudeNewSessionCommand`, `claudeResumeCommand`)
- `src/lib/utils/uid.ts` — Shared UUID generator
- `src/types/workbench.ts` — Shared TypeScript types

### Key path aliases

Defined in `vite.config.ts`:

- `$lib` → `src/lib`
- `$components` → `src/lib/components`
- `$stores` → `src/lib/stores`
- `$types` → `src/types`

### Styling

Tailwind CSS v4 via `@tailwindcss/vite` plugin. UI components from shadcn-svelte (configured in `components.json`, base color: slate). Dark mode forced on.

### Terminal persistence

All workspace and tab TerminalGrids are rendered simultaneously and hidden with `class:hidden` when inactive. This keeps xterm.js instances mounted and PTY processes alive across tab/workspace switches. The `active` prop triggers `fitAddon.fit()` via `$effect` + `requestAnimationFrame` when a tab becomes visible (ResizeObserver doesn't fire on `display:none` elements).

### Claude CLI integration

- New sessions: `claude` (no flags — CLI assigns its own session ID internally)
- Resume sessions: `claude --resume <uuid>`
- Startup commands are typed into the shell (not executed directly), so the shell stays alive even if the CLI command fails. Detect errors via the terminal data stream, not process exit.
- Session data at `~/.claude/projects/<encoded-path>/<session-id>.jsonl` (path encoding: `/` → `-`)
- JSONL format: each line is a JSON object with `type` ("user"/"assistant"), `message.content[]`, `sessionId`, `timestamp`. First user message text = friendly session label.
- `discover_claude_sessions` Rust command reads JSONL files for both the resume menu and detecting session IDs for newly launched sessions (polled every 2s after launch, via `pollForNewSession`).
- Quiescence detection uses a per-pane debounce on `terminal:data` events (not polling). After 1s of no output, the pane is marked as needing attention.
- Session IDs are never generated by the frontend; they come from JSONL files created by the CLI.

## Notes

- Svelte config is minimal (`svelte.config.js` — just `vitePreprocess`). TypeScript config extends `@tsconfig/svelte/tsconfig.json`.
- Rust types use `#[serde(rename_all = "camelCase")]` to match frontend TypeScript field names.

## Gotchas

- Stores must not import other stores. App.svelte wires inter-store communication via callbacks/props. This prevents circular dependencies.
- PTY startup commands run inside the shell process — CLI errors don't trigger `terminal:exit`. Detect errors by buffering early terminal output.
- `tauri.conf.json` must have `beforeDevCommand` set or `tauri dev` will hang waiting for Vite.
- `.prettierignore` excludes `.claude/`, `src-tauri/target/`, and `*.rs` files.
- Singleton store constructors run at import time. Side effects like `listen()` calls are fine in constructors since the Tauri event system is available immediately. No `startWatching()`-style init methods needed.
