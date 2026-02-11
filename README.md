# Workbench

A desktop terminal manager built with Tauri v2 and Svelte 5. Add local project folders, and each project gets a tabbed workspace with real shell terminals, Claude Code integration, and git worktree support.

## Features

- **Project sidebar** — add, edit, remove, and reorder local project folders
- **Tabbed workspaces** — each project gets its own workspace with multiple terminal tabs
- **Terminal splitting** — horizontal and vertical panes with resizable grid layout
- **Real shell terminals** — native PTY sessions via `portable-pty` + `xterm.js`
- **Claude Code integration** — start new sessions, resume previous ones, and view session history per project
- **Git worktree support** — create and manage worktrees with isolated workspaces per branch
- **Startup commands** — auto-run commands when opening a project
- **VS Code integration** — open any project or worktree in VS Code
- **Persistent config** — projects and workspaces saved to `~/.workbench/`

## Stack

- [Tauri v2](https://v2.tauri.app/) (Rust backend + webview)
- [Svelte 5](https://svelte.dev/) (frontend, runes-based reactivity)
- [xterm.js](https://xtermjs.org/) + `portable-pty` (terminal emulation)
- [Tailwind CSS v4](https://tailwindcss.com/) + [shadcn-svelte](https://www.shadcn-svelte.com/)
- [Bun](https://bun.sh/) (package manager)

## Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (for Tauri backend)
- [Bun](https://bun.sh/) (for frontend dependencies)
- macOS (primary target — Linux/Windows support planned)

## Getting Started

```bash
bun install
bun run dev
```

This runs `tauri dev`, which starts the Vite dev server on port 1420 and opens the Tauri window.

## Commands

| Command          | Description                        |
| ---------------- | ---------------------------------- |
| `bun install`    | Install dependencies               |
| `bun run dev`    | Start dev server + Tauri window    |
| `bun run build`  | Build production `.app` and `.dmg` |
| `bun run check`  | Run `svelte-check` type checking   |
| `bun run lint`   | Run Prettier + ESLint checks       |
| `bun run format` | Auto-format with Prettier          |

## Project Structure

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
    git.rs              # Git CLI wrappers (branches, worktrees)
    pty.rs              # PTY session management
    settings.rs         # Claude Code settings discovery
```

## License

This project is licensed under the [GNU Affero General Public License v3.0](LICENSE).
