# Workspace model

`crates/workbench-core/src/workspace/` holds the one workspace → tab → pane model that every client renders, and every rule that changes it. It is pure: `ops::apply(&mut Model, Command) -> Result<Vec<Effect>>` edits the model and returns the effects for the caller (the server's workspace service) to carry out in order. Only `persist.rs` does IO. The rules live in `ops/lifecycle.rs` (workspaces, sessions, processes) and `ops/layout.rs` (labels, splits, order).

## Model (`model.rs`)

- `Model { workspaces }`
- `Workspace { id, projectPath, projectName, worktreePath?, branch?, renderer: xterm|native, tabs, splitView?, transient }`.
  - Its `cwd()` is the worktree, else the project.
  - Only a worktree workspace has a `branch`; a main checkout's branch is read from git when shown.
  - `transient` marks a workspace opened only to host a session started by path. It closes with its last tab, and an explicit `OpenWorkspace` clears the flag.
- `Tab { id, label, kind, split, panes }`. Every pane in a tab has the tab's kind.
- `Pane { id, kind: shell|claude|codex, sessionId?, previousIds, accountId?, prompt?, command?, codexMode?: tui|appServer }`. `prompt` is held only until the pane's first spawn.

Paths are compared with `same_path`: trailing separators are ignored, and on Windows so are case and `\` vs `/`.

**Not in the model (per device or per process):** the selected workspace, the active tab, focus, split ratios, a Claude pane's Terminal/Chat display, chat drafts, "Take control", and runtime state (terminal id, status, exited, title, busy, waiting). An exit is runtime state: the pane stays with its process marked exited, so the model has no exit command. A Codex pane's mode _is_ in the model, because it picks the process.

## Commands (`command.rs`)

The wire shape is `{"type": "<camelCase name>", ...camelCase fields}`.

| Command                                                                                                                  | Rule                                                                                                                   |
| ------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------- |
| `openWorkspace{projectPath, projectName, worktreePath?, branch?, renderer}`                                              | Reuses the workspace with the same project and worktree. A worktree path equal to the project means the main checkout. |
| `closeWorkspace{workspaceId}` / `closeProject{projectPath}`                                                              | Ends every pane.                                                                                                       |
| `newSession{workspaceId \| projectPath+worktreePath?, kind, resume?, prompt?, accountId?, label?, command?, codexMode?}` | See below.                                                                                                             |
| `closePane{paneId}` / `closeTab{tabId}`                                                                                  | Each closed pane is an End. A tab goes with its last pane, and a split holding it is dropped.                          |
| `restart{tabId}`                                                                                                         | AI tabs only. Stop, then spawn on the same pane ids, session and account. A prompt already sent is never sent again.   |
| `setCodexMode{paneId, mode}`                                                                                             | Stop, then spawn the other process on the same thread.                                                                 |
| `rename{tabId, label}`                                                                                                   | Trimmed; an empty label is refused.                                                                                    |
| `split{tabId, direction}`                                                                                                | See below.                                                                                                             |
| `movePane{paneId, tabId}` / `moveTab{tabId, toTabId}` / `moveWorkspace{...}`                                             | Within one workspace, and into a tab of the pane's kind. A tab emptied by a move goes.                                 |
| `trustFolder{paneId}`                                                                                                    | Claude panes only.                                                                                                     |
| `updateProject{projectPath, newPath, projectName}`                                                                       | A project moved or was renamed: its workspaces follow (`same_path`); worktree paths stay.                              |
| `accountMoved{paneId, accountId}`                                                                                        | Server fold-in: a Claude chat switched account (`""` the default login); later spawns use it.                          |
| `sessionAttached{paneId, sessionId}`                                                                                     | Server fold-in: a Codex thread got its id, or `/resume` switched conversation (clears `previousIds`). See below.       |
| `sessionRekeyed{sessionId, previousIds}`                                                                                 | Server fold-in for `/clear`: the pane on any of `previousIds` follows to the new id and keeps the old ones.            |

**`newSession`:**

- **Which workspace.** By path, the session goes in the exact main or worktree workspace, otherwise in a new transient one. It never goes in the main workspace for a worktree cwd.
- **Resume.** A `resume` of a session a pane already runs (matched by id or `previousIds`, same kind) returns that pane and spawns nothing.
- **Ids and prompts.** Claude gets its session id up front. A resumed session drops `prompt`.
- **Labels.** `Claude N` / `Codex N` count that kind + 1; `Terminal N` counts all tabs + 1.
- **Kind-specific fields.** `command` is shell only; `codexMode` is Codex only.

**`split`:** pairs the tab with the next tab, else the previous one, else a new shell tab. The same direction again unsplits, and another direction turns the split. Refused in a native workspace.

**`sessionAttached` when another pane holds the session:** if a pane of the same kind holds that session (as its id or in its `previousIds`), the command is refused with an error naming that pane. The reporting process is a second one on a live session, and the caller resolves it by stopping that process. The model never shows two panes on one session.

Fold-ins on an unknown pane or session are no-ops, because they can race a close. Commands sent by clients fail on unknown ids.

## Effects

- `spawnShell{paneId, cwd, renderer, command, accountId}`
- `spawnClaude{paneId, cwd, renderer, sessionId, resume, accountId, prompt}`: `resume` says the session may already exist. The launcher still checks its history, as `claude_launch` does today.
- `spawnCodex{paneId, cwd, renderer, sessionId?, mode, prompt}`: a `sessionId` whose thread has nothing on disk starts fresh, and the executor reports the new id with `sessionAttached`.
- `stop{paneId, sessionId}`: the process stops and the pane stays (restart, mode switch).
- `end{paneId, sessionId}`: the pane is gone, so its process and session end for every device.
- `trustFolder{paneId, cwd}`
- `opened{workspaceId, tabId?, paneId?}`: what the command made or found, returned to the client that sent it.
- `persist`: the model changed.

## Invariants

- One pane per live session: a resume returns the pane that holds the session, and `sessionAttached` refuses a session another pane holds.
- A prompt is sent with exactly one spawn. The spawn takes it off the pane, and a restart or mode switch never resends it.
- `stop`/`end` come before the `spawn` that replaces them.
- Pane ids survive restart, mode switch, re-key and moves. Clients key local state on them.
- Every pane in a tab has the tab's kind.
- Every mutation ends with `persist`; a no-op returns no effects.

## Files (`persist.rs`)

`persist::load(dir)` / `persist::save(dir, &WorkspacesFile)` use `workspaces.v2.json`: `{version: 2, workspaces, local}`. `local` holds the desktop's per-device state (`selectedId`, `activeTabIds`, `chatPanes`, `serverTerminalIds`) until the desktop stores it itself.

The new format goes in its own file because older builds read `workspaces.json` strictly, and would lose every workspace on a downgrade if it held v2. So `save` never touches `workspaces.json`. `load` reads `workspaces.v2.json` when present, else migrates `workspaces.json`. After a downgrade, an older build sees the old file as it was when the new format took over.

`version` is read explicitly:

- absent or `1`: the old desktop snapshot, migrated;
- `2`: current;
- anything else (newer, zero, non-integer, a string): an error, never a guess.

The migration does the following:

- Claude `view: "chat"` goes to `local.chatPanes`.
- Codex `view` becomes `codexMode`.
- A Codex launch command is reduced to its prompt argument.
- A Claude pane without an id gets one.
- An empty tab gets a shell pane.
- A tab mixing pane kinds is split into one tab per kind. The tabs keep pane order, and the first keeps the tab's id.
- A main checkout's `branch` is dropped.
- A split naming a missing tab is dropped.
- `liveTerminal` is ignored, since it is runtime state.

Writes go through `paths::atomic_write`.

## Server (`apps/server/src/workspace.rs` + `workspace/`)

`WorkspaceService` lives in `Managers`, so the standalone server and the desktop's embedded one run the same code. `WorkspaceService::persistent()` (the binary's `serve`) keeps the model in the config dir, under an advisory pid lock (`workspaces.v2.lock`: a second process on the same dir runs without saving and warns); `Managers::default()` keeps it in memory (tests, and the desktop until it renders the model in Phase 3, when `ServerControl::new` switches to `persistent()`).

- **Commands.** `POST /workspace/commands` takes one `Command` and answers `{rev, workspaceId?, tabId?, paneId?}` once a snapshot at `rev` includes it, or `{rev, error}` (400). `sessionAttached`/`sessionRekeyed`/`accountMoved` are refused: only the server reports what a process did. A Claude `newSession`'s account is decided as every launch's is (`claude_accounts::for_launch`: a pick, `""` the default login; else the login holding a resumed transcript, the project's own, the active one) and kept on the pane.
- **Effects** (`exec.rs`) run in order on a worker per pane, so a slow one holds up only its pane. Spawns go through `terminal::create_from_body`, so every pane gets the cwd allowlist, the server-built `claude`/`codex` command (`claude_launch`, `codex_launch`), the project's own shell and the server's hook env. A Codex chat (`appServer`) starts through `AgentManager::start`. The Codex "has history" check runs here: a thread with nothing on disk starts fresh. `stop` waits for the process to go; `end` Ends its chat for every client and kills its terminal.
- **Runtime** per pane, never saved: `terminalId`, `status` (`starting` | `needsTrust` | `running` | `exited`), `title`, `busy`, `waiting`, `error`, `generation` (bumped by every spawn, so a client re-attaches after a restart it never saw stop). `fold.rs` reads it from the terminal list and chat summaries (by `paneId`, else session ids) and folds back `/resume` (`sessionAttached`), `/clear` (`sessionRekeyed`) a Codex TUI's thread id (its `notify`) and a chat's account switch (`accountMoved`). A `sessionAttached` the model refuses stops the reporting process. A Codex TUI's `busy` is its terminal's output activity (`TerminalManager`, echo of typing excluded).
- **Events.** `GET /events/workspace` (SSE, `?token=` allowed) sends `snapshot` frames, `{rev, workspaces, persistence, local}` (each pane's runtime merged in; `persistence` `{status: ok|locked|error, message}`; `local`, the desktop's per-device state from the saved file, for it to take over once): one on connect, then one per new `rev`. Snapshots are built at most every 150ms and `rev` only grows when one differs, so every subscriber sees the same `rev` sequence. `ping` after 15s of quiet; the stream ends when its listener is revoked.
- **Older routes.** `DELETE /remote/terminals/:id` and `DELETE /agent/:kind/:id?end=true` close the pane that holds that terminal or session. `/events/home` is unchanged.
- **Boot.** When the first listener binds, the service loads its model (`persist::load`: `workspaces.v2.json`, else the desktop's older `workspaces.json` migrated, so an upgrade keeps every tab) and spawns every pane (`ops::boot`). The desktop's `ServerControl` and the standalone binary both run it `persistent()`. A file that won't load (corrupt, a newer `version`, a failed migration) is copied to `<name>.broken-<unix secs>`, logged as an error, reported as `persistence: error`, and never saved over: the process runs unsaved. When another live process holds `workspaces.v2.lock`, this one runs unsaved too and reports `locked` with that pid. The desktop shows either as a banner and doesn't auto-open a project.
- **Clients.** The desktop renders this model and nothing else (`WorkspaceStore`, see CLAUDE.md "Workspaces are the server's"): it never creates a terminal or starts a chat itself. Local per-device state (selection, active tab, a Claude pane's Terminal/Chat view) lives in its localStorage, keyed by these ids.
- **Native panes** are server terminals like xterm ones (`renderer: native` sets `TerminalMeta.native`); the desktop's SwiftTerm view follows one in-process (`TerminalManager::tap`). No separate host is needed. `openWorkspace{renderer: native}` is refused unless the host shows native views (`Managers::native_views`, the macOS desktop) and the request came through its loopback listener. Stopping a native pane's chat leaves its shell (`AgentManager::own_terminal`); closing the pane ends it.
- **Idle.** Snapshots are rebuilt on a command, or on a process change while panes exist; the 1s tick (busy decay, trust dialogs) runs only while someone subscribes.
