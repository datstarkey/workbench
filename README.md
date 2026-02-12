# Workbench

A desktop terminal manager built with Tauri v2 and Svelte 5. Add local project folders, and each project gets a tabbed workspace with real shell terminals, Claude Code + Codex integration, and git worktree support.

## Installation

Download the latest release from the [Releases page](https://github.com/datstarkey/workbench/releases/latest):

- **macOS (Universal)** — `Workbench_x.x.x_universal.dmg`

Open the `.dmg` and drag Workbench to your Applications folder.

> macOS is the primary target. Linux and Windows support is planned.

## Features

- **Project sidebar** — add, edit, remove, and reorder local project folders
- **Tabbed workspaces** — each project gets its own workspace with multiple terminal tabs
- **Terminal splitting** — horizontal and vertical panes with resizable grid layout
- **Real shell terminals** — native PTY sessions via `portable-pty` + `xterm.js`
- **Claude Code integration** — start new Claude sessions, resume previous ones, and view session history per project
- **Codex integration** — start and resume Codex sessions alongside Claude, with full session discovery
- **Git worktree support** — create and manage worktrees with isolated workspaces per branch
- **Startup commands** — auto-run commands when opening a project
- **VS Code integration** — open any project or worktree in VS Code
- **Claude Code settings** — view and edit Claude settings (user, project, local scopes) directly from Workbench
- **Persistent config** — projects and workspaces saved to `~/.workbench/`

## Stack

- [Tauri v2](https://v2.tauri.app/) (Rust backend + webview)
- [Svelte 5](https://svelte.dev/) (frontend, runes-based reactivity)
- [xterm.js](https://xtermjs.org/) + `portable-pty` (terminal emulation)
- [Tailwind CSS v4](https://tailwindcss.com/) + [shadcn-svelte](https://www.shadcn-svelte.com/)
- [Bun](https://bun.sh/) (package manager)

## Development

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (for Tauri backend)
- [Bun](https://bun.sh/) (for frontend dependencies)
- macOS (primary development target)

### Getting Started

```bash
bun install
bun run dev
```

This runs `tauri dev`, which starts the Vite dev server on port 1420 and opens the Tauri window.

### Commands

| Command                | Description                        |
| ---------------------- | ---------------------------------- |
| `bun install`          | Install dependencies               |
| `bun run dev`          | Start dev server + Tauri window    |
| `bun run build`        | Build production `.app` and `.dmg` |
| `bun run check`        | Run `svelte-check` type checking   |
| `bun run lint`         | Run Prettier + ESLint checks       |
| `bun run format`       | Auto-format with Prettier          |
| `bun run test`         | Run all tests (frontend + Rust)    |
| `bun run test:unit`    | Run frontend unit tests only       |
| `bun run test:component` | Run frontend component tests only |

Rust tests can be run separately:

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

### Project Structure

```
src/                    # Svelte frontend
  lib/
    components/         # Shared UI components (shadcn-svelte)
    features/           # Feature modules (projects, workspaces, terminal, claude, worktrees)
    stores/             # Svelte context-based stores
    utils/              # Shared utilities
  types/                # TypeScript type definitions
src-tauri/              # Rust backend
  src/
    commands.rs         # Tauri IPC command handlers
    config.rs           # Project/workspace persistence
    codex.rs            # Codex session discovery and config
    claude_sessions.rs  # Claude session discovery and hooks
    git.rs              # Git CLI wrappers (branches, worktrees)
    pty.rs              # PTY session management
    settings.rs         # Claude Code settings CRUD
    paths.rs            # Path helpers and atomic file writes
```

## License

This project is licensed under the [GNU Affero General Public License v3.0](LICENSE).
