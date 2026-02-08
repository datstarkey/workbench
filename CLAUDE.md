# CLAUDE.md

## Project Overview

Workbench is a Tauri v2 + Svelte 5 desktop terminal manager. Users add local project folders, and each project gets a tabbed workspace with real shell terminals (via `portable-pty` + `xterm.js`). Config persisted to `~/.workbench/projects.json`, workspaces to `~/.workbench/workspaces.json`.

## Commands

- **Install:** `bun install`
- **Dev:** `bun run dev` (runs `tauri dev` — Vite on port 1420 + Rust backend)
- **Build:** `bun run build` (runs `tauri build` — produces .app and .dmg)
- **Typecheck:** `bun run check` (svelte-check)
- **Lint:** `bun run lint` (prettier --check + eslint)
- **Format:** `bun run format` (prettier --write)

Use **Bun** exclusively — do not introduce npm/yarn/pnpm lockfiles.

## Architecture

### Tauri v2 model

- **Rust backend** (`src-tauri/src/`): Window management, PTY sessions, project config I/O, Tauri commands
- **Frontend** (`src/`): Plain Svelte 5 + Vite (no SvelteKit). Dev: `http://localhost:1420`; prod: `dist/index.html`

### Rust source structure (`src-tauri/src/`)

- `main.rs` — Entry point, calls `workbench_lib::run()`
- `lib.rs` — Plugin registration, `PtyManager` state, command registration
- `types.rs` — Shared types (`ProjectConfig`, `CreateTerminalRequest`, `TerminalDataEvent`, workspace snapshots). All `Serialize`/`Deserialize`
- `config.rs` — Project/workspace persistence (`~/.workbench/`), Claude JSONL session discovery (`~/.claude/`)
- `pty.rs` — `PtyManager` with per-session locking (`SessionMap`). Reader threads (8KB buffer) emit `terminal:data` and self-cleanup on EOF
- `commands.rs` — `#[tauri::command]` handlers for all IPC
- `git.rs` — Git CLI wrappers: branch info, worktree CRUD, branch listing
- `paths.rs` — Path helpers and `atomic_write` utility
- `settings.rs` — Claude Code settings CRUD (user/project scopes), plugin/skill/hook discovery

### Frontend IPC

All IPC uses `invoke()` from `@tauri-apps/api/core` and `listen()` from `@tauri-apps/api/event`. Events from Rust→frontend: `terminal:data`, `terminal:exit`.

### Frontend structure

**Stores** (`src/lib/stores/`): Context-based (`createContext` pairs in `context.ts`). Classes with `$state` runes. App.svelte creates instances and sets context; children use `getXxxStore()` getters. Stores can reference each other via constructor params. Context definitions use `type` imports to avoid circular dependencies.

**Features** (`src/lib/features/`):

- `projects/` — Sidebar, project dialog, project manager
- `workspaces/` — Workspace tabs, landing page
- `terminal/` — Terminal tabs, grid layout, terminal pane (xterm.js)
- `claude/` — Session resume menu
- `worktrees/` — Worktree creation dialog, worktree manager

**Manager stores** (`src/lib/features/*/`): `ProjectManagerStore` and `WorktreeManagerStore` own dialog UI state and multi-step workflows (picker → dialog → validate → save). Data stores handle CRUD; manager stores orchestrate UI flows. Both are in context and use `ConfirmAction<T>` from `$lib/utils/confirm-action.svelte.ts` for confirm-before-delete flows.

**Components** (`src/lib/components/`):

- `ConfirmDialog`, `EmptyState`
- `settings/` — `SettingsSheet` + per-tab components (`SettingsEmptyState`, `SettingsSelect`, `SettingsToggle`, `EditableStringList`)
- `ui/` — shadcn-svelte primitives

**Utils** (`src/lib/utils/`):

- `claude.ts` — CLI command builders with UUID validation (`CLAUDE_NEW_SESSION_COMMAND`, `claudeResumeCommand`)
- `confirm-action.svelte.ts` — Reusable confirm-before-action pattern
- `dialog.ts` — Folder picker
- `format.ts` — `formatSessionDate`, `stripAnsi`
- `path.ts` — `baseName`, `effectivePath`
- `terminal.ts` — PTY IPC wrappers
- `uid.ts`, `vscode.ts` — UUID generation, VS Code opener

**Types**: `src/types/workbench.ts`, `src/types/claude-settings.ts` (typed settings with union literals for enums)

### Path aliases (in `vite.config.ts`)

`$lib` → `src/lib`, `$components` → `src/lib/components`, `$features` → `src/lib/features`, `$stores` → `src/lib/stores`, `$types` → `src/types`

### Styling

Tailwind CSS v4 via `@tailwindcss/vite`. shadcn-svelte components (`components.json`, base color: slate). Dark mode forced on.

### Terminal persistence

All TerminalGrids render simultaneously, hidden via `class:hidden` when inactive (keeps xterm.js mounted and PTY alive). The `active` prop triggers `fitAddon.fit()` via `$effect` + `requestAnimationFrame` (ResizeObserver doesn't fire on `display:none`).

### Claude CLI integration

- New sessions: `CLAUDE_NEW_SESSION_COMMAND` constant — just `claude` with no flags (CLI assigns session ID)
- Resume sessions: `claudeResumeCommand(sessionId)` → `claude --resume <uuid>` (validates UUID before shell interpolation)
- Commands typed into shell (not executed directly) — shell stays alive if CLI fails. Detect errors via terminal data stream, not process exit.
- Session data: `~/.claude/projects/<encoded-path>/<session-id>.jsonl` (path encoding: `/` → `-`)
- JSONL format: JSON objects with `type` ("user"/"assistant"), `message.content[]`, `sessionId`, `timestamp`. First user message = session label.
- Runtime session/activity updates are event-driven (`claude:hook` and `codex:notify`); JSONL discovery is on-demand for resume/history and label enrichment.
- Quiescence: per-pane debounce on `terminal:data` events. After 1s of no output, pane marked as needing attention.
- Session IDs come from JSONL files created by the CLI, never generated by frontend.

### Git worktree support

- Workspaces have optional `worktreePath` and `branch`. When `worktreePath` is set, terminals and Claude sessions use it as cwd.
- `effectivePath(ws)` returns `ws.worktreePath ?? ws.projectPath` — use everywhere a workspace cwd is needed.
- Multiple workspaces share the same `projectPath` (one main + N worktrees). `getByProjectPath()` returns only main workspace.
- Sidebar nests worktrees under parent project. Git state lives in `GitStore` (`branchByProject`, `worktreesByProject`), accessed via context.
- `closeAllForProject()` closes main + all worktree workspaces.

## Gotchas

- Rust modules use `anyhow::Result` internally. `commands.rs` converts to `Result<_, String>` for Tauri IPC via `.map_err(|e| e.to_string())`.
- Config/settings writes use `paths::atomic_write()` (temp file + rename) to prevent corruption.
- `PtyManager` uses per-session `Arc<Mutex<PtySession>>` — outer map lock held only briefly for insert/remove/lookup, never during I/O.
- Reader threads self-cleanup on EOF: remove session from map, emit `terminal:exit`. `kill()` handles already-cleaned-up sessions.
- Mutex locks use `.unwrap_or_else(|e| e.into_inner())` to recover from poisoning.
- PTY startup commands run inside the shell — CLI errors don't trigger `terminal:exit`. Detect errors by buffering early terminal output.
- `tauri.conf.json` must have `beforeDevCommand` set or `tauri dev` hangs waiting for Vite.
- `.prettierignore` excludes `.claude/`, `src-tauri/target/`, and `*.rs`.
- Rust types use `#[serde(rename_all = "camelCase")]` to match frontend field names.
- Store constructors run at import time. Side effects like `listen()` are fine — Tauri event system is available immediately.
