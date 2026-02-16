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
- `config.rs` — Project/workspace/settings persistence (`~/.workbench/`), Claude JSONL session discovery (`~/.claude/`)
- `pty.rs` — `PtyManager` with per-session locking (`SessionMap`). Reader threads (8KB buffer) emit `terminal:data` and self-cleanup on EOF
- `commands.rs` — `#[tauri::command]` handlers for all IPC
- `git.rs` — Git CLI wrappers: branch info, worktree CRUD, branch listing
- `paths.rs` — Path helpers and `atomic_write` utility
- `settings.rs` — Claude Code settings CRUD (user/project scopes), plugin/skill/hook discovery
- `github.rs` — GitHub CLI wrappers: remote detection, PR listing, workflow runs, check details
- `trello/` — Trello API integration: `types.rs` (board/card/config types), `api.rs` (reqwest HTTP calls), `config.rs` (persistence to `~/.workbench/trello/`)
- `trello_commands.rs` — Trello IPC command handlers

### Frontend IPC

All IPC uses `invoke()` from `@tauri-apps/api/core` and `listen()` from `@tauri-apps/api/event`. Events from Rust→frontend: `terminal:data`, `terminal:exit`.

### Frontend structure

**Stores** (`src/lib/stores/`): Context-based (`createContext` pairs in `context.ts`). Classes with `$state` runes. App.svelte creates instances and sets context sequentially; children use `getXxxStore()` getters. Stores created later in the sequence can call `getXxxStore()` in field initializers instead of receiving constructor params — avoids argument drilling.

**Features** (`src/lib/features/`):

- `projects/` — Sidebar, project dialog, project manager
- `workspaces/` — Workspace tabs, landing page
- `terminal/` — Terminal tabs, grid layout, terminal pane (xterm.js)
- `claude/` — Session resume menu
- `worktrees/` — Worktree creation dialog, worktree manager
- `trello/` — Board panel, task cards, quick-add, link dialog
- `sidebar/` — Tabbed right sidebar (GitHub | Boards)

**Manager stores** (`src/lib/features/*/`): `ProjectManagerStore` and `WorktreeManagerStore` own dialog UI state and multi-step workflows (picker → dialog → validate → save). Data stores handle CRUD; manager stores orchestrate UI flows. Both are in context and use `ConfirmAction<T>` from `$lib/utils/confirm-action.svelte.ts` for confirm-before-delete flows.

**Components** (`src/lib/components/`):

- `ConfirmDialog`, `EmptyState`
- `settings/` — `SettingsSheet` (Workbench/Claude Code/Integrations tabs) + per-tab components (`SettingsWorkbench`, `SettingsEmptyState`, `SettingsSelect`, `SettingsToggle`, `EditableStringList`, `SettingsTrelloAuth`, `SettingsBoardConfig`)
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
- Commands typed into shell (not executed directly) — CLI errors don't trigger `terminal:exit`. Detect errors by buffering early terminal output, not process exit.
- Session data: `~/.claude/projects/<encoded-path>/<session-id>.jsonl` (path encoding: `/` → `-`)
- JSONL format: JSON objects with `type` ("user"/"assistant"), `message.content[]`, `sessionId`, `timestamp`. First user message = session label.
- Runtime session/activity updates are event-driven (`claude:hook` and `codex:notify`); JSONL discovery is on-demand for resume/history and label enrichment.
- Quiescence: per-pane debounce on `terminal:data` events. After 1s of no output, pane marked as needing attention.
- Session IDs come from JSONL files created by the CLI, never generated by frontend.

### Git worktree support

- Workspaces have optional `worktreePath` and `branch`. Main workspaces resolve branch from `gitStore.branchByProject` at read time (no sync needed). Worktree workspaces get `branch` set at creation time. When `worktreePath` is set, terminals and Claude sessions use it as cwd.
- `effectivePath(ws)` returns `ws.worktreePath ?? ws.projectPath` — use everywhere a workspace cwd is needed.
- Multiple workspaces share the same `projectPath` (one main + N worktrees). `getByProjectPath()` returns only main workspace.
- Sidebar nests worktrees under parent project. Git state lives in `GitStore` (`branchByProject`, `worktreesByProject`), accessed via context.
- `closeAllForProject()` closes main + all worktree workspaces.
- Worktree location strategy (workbench setting): `"sibling"` creates `<parent>/<repo>-<branch>`, `"inside"` creates `<repo>/.worktrees/<branch>` (auto-adds to `.gitignore`).
- `WorkbenchSettingsStore` manages `~/.workbench/settings.json` — single-scope store (vs multi-scope `ClaudeSettingsStore`).

### GitHub CI integration

- `GitHubStore` polls project status (PRs, workflow runs, PR checks) on 90s cycle, 15s fast-poll when pending.
- Polling is scoped to projects with active Claude/Codex sessions (`activeSessionsByProject`), not all open workspaces.
- `get_project_status()` batches all data in one IPC call: PRs, workflow runs grouped by branch, and pre-fetched PR checks for open PRs.
- `gh run list` returns `databaseId` (not `id`) — use `#[serde(alias = "databaseId")]` for deserialization.
- `gh pr checks` returns empty string `""` for `completedAt` on pending checks — validate parsed dates before computing durations.

### Svelte 5 reactivity

**`$derived` is king** — use for all computed state. `$derived.by` for complex expressions. Writable `$derived` (Svelte 5.25+) for computed values that can be temporarily overridden.

**`$effect` is an escape hatch** — only for external side effects like network requests, DOM manipulation, and analytics. Per the Svelte docs: "In general, `$effect` is best considered something of an escape hatch — useful for things like analytics and direct DOM manipulation — rather than a tool you should use frequently. In particular, avoid using it to synchronise state."

**Never use `$effect` to sync state.** Don't read reactive values in an effect and write them to other reactive state. Instead, use `$derived` or `$derived.by`. If you need a computed value that can be reassigned (optimistic UI), use writable `$derived` instead of an effect.

**Cross-component reactive state:** Derive in the store (where the data lives), not via effects in components. If multiple components need the same derived value, put the `$derived` on the store class, not in each component.

**Cross-store derived state:** Put the `$derived` on the store that owns the concept (e.g., `activeBranches` on `GitHubStore`), not as an `$effect` in a component that reads one store and writes to another.

## Gotchas

- Rust modules use `anyhow::Result` internally. `commands.rs` converts to `Result<_, String>` for Tauri IPC via `.map_err(|e| e.to_string())`.
- Config/settings writes use `paths::atomic_write()` (temp file + rename) to prevent corruption.
- `PtyManager` uses per-session `Arc<Mutex<PtySession>>` — outer map lock held only briefly for insert/remove/lookup, never during I/O.
- Reader threads self-cleanup on EOF: remove session from map, emit `terminal:exit`. `kill()` handles already-cleaned-up sessions.
- Mutex locks use `.unwrap_or_else(|e| e.into_inner())` to recover from poisoning.
- `tauri.conf.json` must have `beforeDevCommand` set or `tauri dev` hangs waiting for Vite.
- `.prettierignore` excludes `.claude/`, `src-tauri/target/`, `src-tauri/gen/`, and `*.rs`.
- Rust types use `#[serde(rename_all = "camelCase")]` to match frontend field names.
- Store constructors run at import time. Side effects like `listen()` are fine — Tauri event system is available immediately.
- `ConfirmDialog` delegates close behavior to the bound `ConfirmAction.open` — don't auto-close on confirm (allows async error display + retry).
- Svelte 5 `$state` with union types: use `$state<'a' | 'b'>('a')` not `let x: 'a' | 'b' = $state('a')` — the latter narrows to the initial value's literal type.
- `$derived` on class fields is lazy — the callback runs on first read, not at field initialization time. Safe to reference constructor params that are set after field initializers run.
- **Context init order in App.svelte is load-bearing.** Stores that call `getXxxStore()` in field initializers will crash at runtime (`missing_context`) if the dependency hasn't been `setXxxStore()`'d yet. `GitHubStore` depends on `WorkspaceStore`, `GitStore`, `ClaudeSessionStore`, `ProjectStore`. Always verify the full dependency graph before reordering.
- When making a sync store method async, grep for all callers in `*.test.svelte.ts` — tests that don't `await` the call silently pass without verifying behavior.
- When gating a code path (e.g., adding approval before session launch), trace **all** callers of the underlying method — sidebar context menus, landing pages, terminal tabs, etc. may bypass the new gate.
- Dialogs using `bind:open` let X/Escape/outside-click close without resolving pending promises. Use `onOpenChange` to intercept dismissal when the dialog controls async flow.
- Every frontend `invoke('command_name')` call needs three things: (1) the `async fn` in a `_commands.rs` file, (2) registration in `lib.rs`'s `invoke_handler`, and (3) matching `#[serde(rename_all = "camelCase")]` types. Missing any one silently fails at runtime.
- Per-project config (like `TrelloProjectConfig`) must be loaded at startup in `App.svelte`'s `onMount`, not only from settings UI. Otherwise features depending on that config (sidebar panels, merge automation) won't work until settings is opened.
- Dialog pre-fill from props: don't use `$state(prop)` (captures initial value only). Instead, apply prop values in the dialog's `onOpenChange` callback when `isOpen` is true.
- `main` branch has force-push protection. Always use feature branches for multi-step changes; don't amend already-pushed commits to `main`.
