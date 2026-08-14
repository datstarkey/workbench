# CLAUDE.md

## Project Overview

Workbench is a Tauri v2 + Svelte 5 desktop terminal manager. Users add local project folders, and each project gets a tabbed workspace with real shell terminals (via `portable-pty` + `xterm.js`). Config persisted to `~/.workbench/projects.json`, workspaces to `~/.workbench/workspaces.json`.

It is a **monorepo** (Cargo workspace + bun/Turborepo workspace). Beyond the desktop app it ships a **headless control-plane server** (`workbench-server`) that can run on any machine — and the desktop app can host the same server in "server mode". The server lets another device (phone/desktop) list/create worktrees and **spawn `claude remote-control` sessions** remotely; those sessions register with Anthropic's API and appear in the Claude mobile app / claude.ai automatically. The server never proxies terminal IO — only the control plane crosses the network.

### Monorepo layout

```
Cargo.toml / package.json / turbo.json   # workspace roots (repo root)
apps/
  desktop/      # the Tauri app: src/ (Svelte frontend) + src-tauri/ (Rust, crate `workbench`)
  server/       # workbench-server: headless axum control plane (lib + bin), NO tauri
  mobile/       # placeholder (Tauri-mobile client, not yet built)
crates/
  workbench-core/   # shared pure Rust logic (no tauri): config, git, sessions, settings, types
packages/
  types/             # @workbench/types: shared TS types (mirror of workbench-core serde types)
  transport/         # @workbench/transport: ControlPlaneTransport (Tauri/Http/mock impls), typed via @workbench/types
  ui/                # @workbench/ui: shadcn-svelte primitives + cn(); subpath exports (@workbench/ui/button …)
  control-plane-ui/  # @workbench/control-plane-ui: transport-driven ControlPlaneStore + ControlPlaneSidebar (mobile / desktop-remote)
```

`@workbench/ui` owns the shadcn primitives (`@workbench/ui/<component>`) and `cn`/type-helpers (`@workbench/ui` root). Apps must add `@source '<rel>/packages/ui/src'` in their `app.css` so Tailwind v4 generates classes used only in the package. `@workbench/control-plane-ui` is a clean, transport-driven shared sidebar (list projects, view/create worktrees, spawn/kill remote sessions, no terminal IO) — consumed by the desktop's **remote-client mode** (ActivityRail "Remote server" → `RemoteServerDialog`, `HttpTransport`) and, in future, the native mobile app.

Not yet extracted (deferred; desktop keeps its own terminal-coupled `ProjectSidebar`/stores): `@workbench/tailwind-preset`, and splitting the desktop `ClaudeSessions`/`Workspace` stores into shared core slices. The desktop's rich sidebar is intentionally NOT shared — it's coupled to terminals/git/github; the shared `ControlPlaneSidebar` is the lean cross-platform one.

Three Cargo crates: `workbench` (desktop, depends on core + server), `workbench-core` (pure logic), `workbench-server` (lib+bin). `workbench-core` and `workbench-server` must **never** depend on `tauri` — verify with `cargo tree -p workbench-server | grep -i tauri` (must be empty). Tauri deps live only in `apps/desktop/src-tauri/Cargo.toml`.

## Release

- **GitHub releases:** created automatically by GitHub Actions (`tauri-action`) when a `v*` tag is pushed. **Do not** run `gh release create` manually — it causes duplicate/empty releases.
- **Issue/PR linking:** enabled — append `(#N)` references to changelog entries so GitHub auto-links them.
- **Tag format:** `v`-prefixed semver (e.g. `v0.18.0`).
- **Changelog location:** `changelog/RELEASE_{version}.md`.
- **Release flow:** write changelog → commit → tag → push commit + tag → let CI publish the release.
- **macOS signing/notarization:** see `docs/SIGNING.md`. Driven by `APPLE_*` GitHub secrets; the release workflow degrades to an unsigned build (with a CI warning) when they're absent. Local signed builds: `bun run --cwd apps/desktop build:signed` + a `signing.env` at the repo root.

## Workflow

**Default flow for all non-trivial work** — never commit directly to `main`:

1. **Branch:** create a feature branch off `main` (e.g. `feat/...`, `fix/...`). `main` has force-push protection.
2. **Commit:** Conventional Commits format. Commit/push only when the change is complete or the user asks.
3. **PR:** open a PR with `gh pr create`. Reference issues/PRs with `(#N)` so GitHub auto-links.
4. **Auto-merge:** enable auto-merge (`gh pr merge --auto --squash`) so the PR lands once CI passes — don't sit and babysit checks unless asked.
5. **Cleanup:** delete the branch after merge (auto-merge with `--squash` does this on GitHub).

The `github-pr` / `prep-pr` skills automate this; prefer them over hand-running `gh`.

## Commands

Run from the repo root. JS tasks go through Turborepo (the local `turbo` binary — invoke via `bun run`, not a global `turbo`).

- **Install:** `bun install` (bun workspaces link `@workbench/*` into the consuming app's `node_modules`)
- **Dev:** `bun run dev` (`turbo run dev --filter=@workbench/desktop` → `tauri dev`, Vite on 1420)
- **Build:** `bun run build` (`turbo run build`)
- **Typecheck:** `bun run check` (`turbo run check` — svelte-check on desktop, tsc on packages)
- **Lint:** `bun run lint` (root `prettier --check .` then `turbo run lint` — eslint per package)
- **Format:** `bun run format` (`prettier --write .`)
- **Rust:** `cargo build` / `cargo test` / `cargo clippy` operate on the whole workspace; scope with `-p workbench|workbench-core|workbench-server`.
- **Server (standalone):** `cargo run -p workbench-server -- --port 4317` (flags: `--bind`, `--port`, `--token`; envs `WORKBENCH_BIND/PORT/TOKEN`).

### Tests

Maps onto Tauri v2's three testing layers (https://v2.tauri.app/develop/tests/): **(1)** mock the frontend IPC boundary, **(2)** test Rust `#[tauri::command]`s under the mock runtime, **(3)** WebDriver e2e for the GUI.

- **Layer 1 — frontend IPC.** Control-plane code depends on the `ControlPlaneTransport` abstraction, so tests **inject `createMockTransport()`** (dependency inversion — preferred over monkey-patching `mockIPC`). Direct `@tauri-apps/api` paths (terminal IO/events) are intercepted by `apps/desktop/src/test/tauri-mocks.ts` (`vi.mock('@tauri-apps/api/{core,event}')` exposing `listenSpy` / `emitMockEvent` / `clearListeners`) — our hand-rolled equivalent of `@tauri-apps/api/mocks`.
- **Layer 2 — Rust commands via the mock runtime.** `tauri::test` (`mock_builder().manage(state).build(mock_context(noop_assets()))`) drives `#[tauri::command]`s with no webview. Enabled by `tauri = { features = ["test"] }` declared as a **dev-dependency** (`apps/desktop/src-tauri/Cargo.toml`) so feature unification keeps `test` out of release builds. Example: `server_control.rs`'s `start_status_stop_cycle` exercises the start/stop/status commands + the double-checked-lock race guard. Thin wrapper commands (`commands.rs` etc.) stay untested at this layer — their logic lives in `workbench-core` (unit-tested).
- **Layer 3 — WebDriver e2e: not used.** Tauri's desktop WebDriver (`tauri-driver`) supports only Linux/Windows — **macOS has no WKWebView driver**, and this is a macOS-first app. So the rendered GUI is not e2e-tested.
- **Rust (`cargo test`):** `workbench-core` units, the `workbench` desktop crate units (`pty`, dispatchers, `server_control` mock-runtime), `workbench-server` units, and `apps/server/tests/server.rs` (e2e: real server on an ephemeral port, driven with reqwest + a `tokio-tungstenite` WS client). The e2e suite covers spawn→list→kill, idempotent `remote_kill` (204 not 500), unregistered-cwd rejection, session/terminal caps, terminal-WS `?token=` auth, and WS-close on kill **and** on shell exit.
- **Test env knobs** (read by the server/core at runtime): `WORKBENCH_CLAUDE_BIN` (fake `claude`), `WORKBENCH_CONFIG_DIR` (isolate `~/.workbench`; write a `projects.json` so the spawn/terminal **cwd allowlist** accepts the test dir — see `register_project()`), `WORKBENCH_MAX_SESSIONS` / `WORKBENCH_MAX_TERMINALS` (low caps for the cap tests). These mutate **process-global** env, so env-mutating tests hold the module `ENV_LOCK` mutex (`env_guard()`) and only one test may depend on a given value at a time.
- **JS (`turbo run test`):** `@workbench/transport` (HTTP route mapping, undefined-param dropping, error handling, `MockTransport`, `/events` reconnect backoff, **and the `TauriTransport` shim** — `tauri.test.ts` mocks `@tauri-apps/api/{core,event}` to assert the omit-args-when-undefined call shape + listener wiring), `@workbench/control-plane-ui` (`ControlPlaneStore` via `createMockTransport`, incl. spawn polling + `dispose()` with fake timers), `@workbench/desktop` (stores/utils via injected `createMockTransport`; `InstancesStore`/`RemoteInstance`), and **`@workbench/mobile`** (jsdom vitest: `MobileClient` in `client.svelte.ts` — extracted from `App.svelte` so the connection/terminal logic is unit-testable — and the pure `terminalWsUrl` helper). Mobile component rendering (xterm) is intentionally not tested; the logic lives in the store/helper.
- **Cross-language integration** (`packages/transport/src/http.integration.test.ts`): drives the real `HttpTransport` against the real `workbench-server` binary (spawn→list→kill; registers its project dir via `WORKBENCH_CONFIG_DIR`). Auto-**skips** unless `target/debug/workbench-server` exists, so build it first (`cargo build -p workbench-server`); otherwise `turbo test` silently skips it.
- **Deliberately not unit-tested — this is the WebDriver layer's job (blocked on macOS):** rendered `.svelte` component interaction. Our components are thin context-coupled views that delegate to the thoroughly-tested stores, and they wrap Bits UI / shadcn primitives (dialog portals, focus traps) that are friction-prone and flaky under jsdom. So the policy is: **push real logic out of components into stores/helpers and unit-test that** (e.g. `MobileClient`, `terminalWsUrl`), rather than render-test the views. Component _rendering_ is verified by the WebDriver e2e layer, which macOS can't run.
- Also not covered: the `terminal.rs` attach lock-ordering reattach race (timing-only) and the `spawn_blocking` offloading (behavior-preserving — no observable change to assert).

Use **Bun** exclusively — do not introduce npm/yarn/pnpm lockfiles. Prettier runs at the root (single pass, keeps prettier-before-eslint ordering); eslint runs per package via turbo.

## Architecture

### Tauri v2 model

- **Rust backend** (`apps/desktop/src-tauri/src/`): Window management, PTY sessions, Tauri commands, embedded server control. Crate `workbench` (lib name `workbench_lib`).
- **Frontend** (`apps/desktop/src/`): Plain Svelte 5 + Vite (no SvelteKit). Dev: `http://localhost:1420`; prod: `apps/desktop/dist/index.html` (tauri `frontendDist` is `../dist` relative to `src-tauri/`).

### Shared core crate (`crates/workbench-core/src/`)

Pure, non-Tauri logic reused by desktop and server. All `Serialize`/`Deserialize`, all return `anyhow::Result`:

- `types.rs` — Shared serde types (`ProjectConfig`, `CreateWorktreeRequest`, `WorkbenchSettings`, workspace snapshots, GitHub/Trello types). The single wire contract for both Tauri IPC and the server's JSON API (`#[serde(rename_all = "camelCase")]`).
- `config.rs` — Project/workspace/settings persistence (`~/.workbench/`)
- `git.rs` — Git CLI wrappers: branch info, worktree CRUD, branch listing
- `claude_sessions.rs` / `codex_sessions.rs` / `codex_config.rs` — session discovery (`~/.claude/`, `~/.codex/`)
- `settings.rs` — Claude Code settings CRUD, plugin/skill/hook discovery
- `github.rs` — GitHub CLI wrappers
- `trello/` + `trello_automation.rs` — Trello API integration (async; uses a self-built Tokio runtime, no Tauri)
- `paths.rs` (`atomic_write`), `session_utils.rs`, `shell_integration.rs`

Desktop re-exports every core module at its crate root (`pub use workbench_core::{config, git, types, …}` in `lib.rs`) so `crate::config`, `crate::types`, etc. resolve unchanged across the desktop code — only the re-export line changes when moving logic to core.

### Desktop-only Rust (`apps/desktop/src-tauri/src/`)

- `main.rs` / `lib.rs` — entry, plugin registration, `build_invoke_handler!` macro, state management
- `commands.rs`, `git_commands.rs`, `trello_commands.rs` — thin `#[tauri::command]` shells calling `workbench_core`
- `pty.rs` — `PtyManager` (reader threads emit `terminal:data`, self-cleanup on EOF)
- `hook_bridge.rs`, `git_watcher.rs`, `github_poller.rs`, `refresh_dispatcher.rs`, `menu.rs`, `native_terminal*` (macOS)
- `server_control.rs` — `start_server`/`stop_server`/`server_status` commands; embeds `workbench_server` via `spawn_embedded`, holds the `ServerHandle` in managed state

### Server crate (`apps/server/src/`)

`workbench-server` (lib + bin). axum control plane; **no tauri**.

- `lib.rs` — `app()` (router + auth + CORS), `serve()` (standalone), `spawn_embedded()`/`ServerHandle` (for desktop server mode)
- `routes.rs` — control-plane endpoints (1:1 with core ops): `GET /projects`, `GET/POST/DELETE /projects/worktrees`, `GET /projects/branches`, `GET /projects/git-info`, `GET /sessions/{claude,codex}`, `GET /settings/{claude,workbench}`, `PUT /settings/sync` (501 stub seam), `POST /remote/spawn`, `GET /remote/sessions`, `DELETE /remote/sessions/:id`, `GET /health`
- `spawn.rs` — `RemoteControlManager`: PTY-backed `claude remote-control` spawner. Resolves+validates cwd against `git::list_worktrees`, keeps the process alive, scans stdout for the session URL, lists/kills. **Claude only** (Codex has no remote-control). No IO proxying.
- `auth.rs` — optional bearer-token middleware (no-op unless `--token` set; `/health` exempt; constant-time compare)
- `error.rs` (`ApiError`: anyhow→HTTP), `state.rs` (`AppState`), `cli.rs` (clap)
- Unauthenticated by default; binds `0.0.0.0`; secure with a private network (Tailscale).

### Frontend IPC & transport abstraction

Control-plane stores call `invoke`/`listen` imported from **`$lib/transport`** (not `@tauri-apps/api` directly). That module holds the active `ControlPlaneTransport` (`@workbench/transport`): `createTauriTransport()` (default, local IPC) or `createHttpTransport()` (remote server). The shims keep Tauri-compatible signatures so call sites are unchanged. **Terminal IO** (`create/write/resize/kill_terminal`, native terminal), native dialogs, window/menu, and hook/terminal events stay on **direct `@tauri-apps/api`** — desktop-local, never routed remotely. `transport().capabilities.terminalIO` feature-detects local vs remote.

`@workbench/transport` is the only place `@tauri-apps/api` may be imported among shared packages (in `tauri.ts`, an optional peer dep). Events from Rust→frontend: `terminal:data`, `terminal:exit` (local); control-plane events `project:refresh-requested`, `claude:hook`, `codex:notify`.

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

### Path aliases (in `apps/desktop/vite.config.ts`)

`$lib` → `src/lib`, `$components` → `src/lib/components`, `$features` → `src/lib/features`, `$stores` → `src/lib/stores`, `$types` → `../../packages/types/src` (the `@workbench/types` package — repointed so existing `$types/workbench` imports work unchanged). All resolved relative to `apps/desktop`, cwd-relative — turbo runs vite from `apps/desktop`. Cross-package imports use the package name (`@workbench/transport`, `@workbench/types`), not `$`-aliases. Frontend feature/store/component paths below are all under `apps/desktop/src/`.

### Styling

Tailwind CSS v4 via `@tailwindcss/vite`. shadcn-svelte components (`components.json`, base color: slate). Dark mode forced on.

- Pro chrome tokens: `--wb-*` CSS vars in `app.css` (`bg`/`panel`/`panel2`/`rail`/`ink`/`hair` + session colors `claude`/`codex`/`shell`/`ok`/`warn`/`err`), exposed as Tailwind utilities (`bg-wb-panel`, `text-wb-claude`). Use these for new chrome instead of shadcn surface tokens (keeps dialogs/inputs unbroken).
- Accent is theme-selectable: `:root[data-accent='violet|tideline|ember|moss|iris']` presets in `app.css` drive `--wb-accent` + shadcn `--primary`; the attribute is set from `workbenchSettingsStore.accentColor`. Add new presets in `app.css` AND the swatch list in `SettingsWorkbench.svelte`.
- Window chrome components live in `src/lib/features/chrome/` (`ActivityRail` 44px left rail, `StatusBar` 22px bottom). App uses the native titlebar (`decorations: true`) — no custom traffic-light bar.

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
- `gh repo list --json` valid fields include `name,nameWithOwner,description,isPrivate,isFork,url,sshUrl` — `httpCloneUrl` does **not** exist and causes `gh` to exit with an error. The `url` (web URL) also works as an HTTP clone URL.
- `gh pr checks` returns empty string `""` for `completedAt` on pending checks — validate parsed dates before computing durations.

### Observability (Sentry)

- **Self-hosted Sentry** at `sentry.starkeydigital.com`. Manage via the `sentry` skill / `sentry-admin` CLI.
  - **Org:** `starkey-digital`
  - **Project:** `workbench` (numeric id `20`)
  - Example: `sentry-admin issues starkey-digital -p workbench`
- **Errors only** on both ends (no tracing — sample rate `0`). Same DSN/project for frontend + backend.
- **Frontend:** `@sentry/svelte`. `src/lib/sentry.ts` exports `initSentry()`; `src/main.ts` calls it before mount for **both** the main and settings windows. Prod-gated — no-op unless `import.meta.env.PROD`, and an explicit early-return when `import.meta.env.MODE === 'test'`. DSN is a public write-only key inlined in `sentry.ts`, overridable via `VITE_SENTRY_DSN`. Release tag `workbench@${__APP_VERSION__}` (`__APP_VERSION__` from `vite.config.ts` ← `tauri.conf.json` version).
- **Backend (Rust):** `src-tauri/src/observability.rs` exports `init(release)`, called once at the top of `lib.rs::run()` with the Tauri package version; the returned guard is held for the process lifetime (drop flushes). Captures Rust **panics** (incl. background threads) the frontend can't see. DSN inlined, overridable via `SENTRY_DSN` at build time. Release tag `workbench@<version>`.
- **Prod/test gating (backend):** the Sentry client is a no-op when `cfg!(debug_assertions) || cfg!(test)` — so `cargo test`, `cargo test --release`, and `tauri dev` never report upstream.
- **Backend logging:** `observability::init` also installs a global `log` logger (`env_logger` → stderr, respects `RUST_LOG`, default `info`) wrapped by Sentry. In release: `log::error!` → Sentry **event**, `log::warn!`/`log::info!` → **breadcrumb** (shown on the next event). Use `log::warn!`/`log::error!` for backend diagnostics, **not** `eprintln!`. Stderr output works in all builds; Sentry forwarding only in release.

### Svelte 5 reactivity

**`$derived` is king** — use for all computed state. `$derived.by` for complex expressions. Writable `$derived` (Svelte 5.25+) for computed values that can be temporarily overridden.

**`$effect` is an escape hatch** — only for external side effects like network requests, DOM manipulation, and analytics. Per the Svelte docs: "In general, `$effect` is best considered something of an escape hatch — useful for things like analytics and direct DOM manipulation — rather than a tool you should use frequently. In particular, avoid using it to synchronise state."

**Never use `$effect` to sync state.** Don't read reactive values in an effect and write them to other reactive state. Instead, use `$derived` or `$derived.by`. If you need a computed value that can be reassigned (optimistic UI), use writable `$derived` instead of an effect.

**`runed` is available** — for unavoidable side effects prefer `watch(() => dep, (v) => {…})` (tracked dependency getter, untracked callback) over `$effect`. Don't refactor pre-existing `$effect`s unless asked.

**Cross-component reactive state:** Derive in the store (where the data lives), not via effects in components. If multiple components need the same derived value, put the `$derived` on the store class, not in each component.

**Cross-store derived state:** Put the `$derived` on the store that owns the concept (e.g., `activeBranches` on `GitHubStore`), not as an `$effect` in a component that reads one store and writes to another.

## Gotchas

### Monorepo

- **Pure logic goes in `workbench-core`, not desktop.** When adding shared logic (git, config, sessions, settings), put it in `crates/workbench-core` and (if it needs a Tauri command) add a thin wrapper in `apps/desktop/src-tauri/src/commands.rs`. Never add `tauri` to core or server; re-export new core modules at the desktop crate root in `lib.rs`.
- **Adding a server endpoint** = add the route in `apps/server/src/routes.rs` calling a `workbench_core` fn, reuse a `types.rs` struct for the body/response (camelCase so it matches frontend), and (if the desktop/mobile client should call it) add the command name to `ControlPlaneCommands` + the route mapping in `packages/transport/src/http.ts`.
- **Control-plane stores import `invoke`/`listen` from `$lib/transport`**, not `@tauri-apps/api`. Terminal IO / native dialogs / window APIs stay on direct `@tauri-apps/api`. Don't route terminal IO through the transport — it's local-only.
- `@tauri-apps/api` must not be imported in `packages/*` except `transport/src/tauri.ts` (optional peer dep) — keeps shared packages buildable by the future mobile app.
- bun workspaces nest `@workbench/*` symlinks under the **consumer's** `node_modules` (e.g. `apps/desktop/node_modules/@workbench/transport`), not hoisted to root. A new workspace dep needs declaring in the consumer's `package.json` (`"workspace:*"`) + `bun install`.
- `TauriTransport` uses **static** imports of `@tauri-apps/api` (not dynamic) so call timing/arg-shape match what stores/tests expect; it omits the args object when undefined.
- UI primitives import from `@workbench/ui/<component>` and `cn` from `@workbench/ui` (NOT `$lib/components/ui` / `$lib/utils` anymore). `components.json` (shadcn CLI) aliases still say `$lib/...`; if you `shadcn add` a component it lands in the app — move it into `packages/ui/src/ui/` and fix its `$lib/utils` import to `../../utils`.
- The server spawns `claude remote-control` via the binary named by `WORKBENCH_CLAUDE_BIN` (default `claude`) — set it to a fake script in tests, or to point at an alternative CLI.
- **Left sidebar is instance-aware** (`$features/instances/`): `InstancesStore` holds the implicit local instance ("This Mac") plus persisted remote servers (localStorage), each remote owning a `ControlPlaneStore` (HttpTransport) + health-polled status. `InstanceSwitcher` (dropdown in the sidebar header) sets `activeId`; `App.svelte` renders the rich terminal-coupled `ProjectSidebar` when local, else `RemoteInstanceSidebar` (sidebar chrome + shared `ControlPlaneSidebar`). `ConnectInstanceDialog` adds a server (name/URL/token + `/health` test). So: control-plane features that should work on remote instances belong in the shared `ControlPlaneSidebar`, not `ProjectSidebar`.
- Adding a `WorkbenchSettings` field touches 5 places: Rust `crates/workbench-core/src/types.rs` (field + `default_*` fn + `Default` impl), TS `apps/desktop/src/types/workbench.ts` interface, store `workbench-settings.svelte.ts` (field decl + `load()` + `toSettings()`), and `workbench-settings.test.svelte.ts` — two exact `toHaveBeenCalledWith('save_workbench_settings', …)` assertions list every field, so both break until updated.
- Rust modules use `anyhow::Result` internally. `commands.rs` converts to `Result<_, String>` for Tauri IPC via `.map_err(|e| e.to_string())`.
- Config/settings writes use `paths::atomic_write()` (temp file + rename) to prevent corruption.
- `PtyManager` uses per-session `Arc<Mutex<PtySession>>` — outer map lock held only briefly for insert/remove/lookup, never during I/O.
- Reader threads self-cleanup on EOF: remove session from map, emit `terminal:exit`. `kill()` handles already-cleaned-up sessions.
- Mutex locks use `.unwrap_or_else(|e| e.into_inner())` to recover from poisoning.
- `tauri.conf.json` must have `beforeDevCommand` set or `tauri dev` hangs waiting for Vite.
- `.prettierignore` (at root) excludes `.claude/`, `/target/`, `apps/desktop/src-tauri/gen/`, `dist/`, `.turbo/`, and `*.rs`. `tailwindStylesheet` in `.prettierrc` points at `./apps/desktop/src/app.css`. eslint config reads `../../.gitignore`.
- Rust types use `#[serde(rename_all = "camelCase")]` to match frontend field names.
- Store constructors run at import time. Side effects like `listen()` are fine — Tauri event system is available immediately.
- `ConfirmDialog` delegates close behavior to the bound `ConfirmAction.open` — don't auto-close on confirm (allows async error display + retry).
- Svelte 5 `$state` with union types: use `$state<'a' | 'b'>('a')` not `let x: 'a' | 'b' = $state('a')` — the latter narrows to the initial value's literal type.
- `$derived` on class fields is lazy — the callback runs on first read, not at field initialization time. Safe to reference constructor params that are set after field initializers run.
- **Context init order in App.svelte is load-bearing.** Stores that call `getXxxStore()` in field initializers will crash at runtime (`missing_context`) if the dependency hasn't been `setXxxStore()`'d yet. `GitHubStore` depends on `WorkspaceStore`, `GitStore`, `ClaudeSessionStore`, `ProjectStore`. Always verify the full dependency graph before reordering.
- **Never call `getXxxStore()` from runtime methods** (async handlers, callbacks, non-init code paths). It uses `hasContext()` which only works during component initialisation → `lifecycle_outside_component` error. Instead, cache the store as a field initializer (`private settingsStore = getWorkbenchSettingsStore()`) and use `this.settingsStore` in methods.
- When making a sync store method async, grep for all callers in `*.test.svelte.ts` — tests that don't `await` the call silently pass without verifying behavior.
- When gating a code path (e.g., adding approval before session launch), trace **all** callers of the underlying method — sidebar context menus, landing pages, terminal tabs, etc. may bypass the new gate.
- Dialogs using `bind:open` let X/Escape/outside-click close without resolving pending promises. Use `onOpenChange` to intercept dismissal when the dialog controls async flow.
- Every frontend `invoke('command_name')` call needs three things: (1) the `async fn` in a `_commands.rs` file, (2) registration in `lib.rs`'s `invoke_handler`, and (3) matching `#[serde(rename_all = "camelCase")]` types. Missing any one silently fails at runtime.
- Per-project config (like `TrelloProjectConfig`) must be loaded at startup in `App.svelte`'s `onMount`, not only from settings UI. Otherwise features depending on that config (sidebar panels, merge automation) won't work until settings is opened.
- Dialog pre-fill from props: don't use `$state(prop)` (captures initial value only). Instead, apply prop values in the dialog's `onOpenChange` callback when `isOpen` is true.
- `main` branch has force-push protection. Always use feature branches for multi-step changes; don't amend already-pushed commits to `main`.
- `ScrollArea` internal viewport uses `size-full` (`h-full`) — `max-h-N` on the root doesn't constrain it. Use a fixed `h-N` for scroll areas that must stay bounded.
