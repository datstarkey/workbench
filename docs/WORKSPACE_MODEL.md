# Workspace model

`crates/workbench-core/src/workspace/` holds the one workspace → tab → pane model that every client renders, and every rule that changes it. It is pure: `ops::apply(&mut Model, Command) -> Result<Vec<Effect>>` edits the model and returns the effects for the caller (the server's workspace service) to carry out in order. Only `persist.rs` does IO.

## Model (`model.rs`)

- `Model { workspaces }`
- `Workspace { id, projectPath, projectName, worktreePath?, branch?, renderer: xterm|native, tabs, splitView?, transient }`. Its `cwd()` is the worktree, else the project. `transient` marks a workspace opened only to host a session started by path; it closes with its last tab, and an explicit `OpenWorkspace` clears the flag.
- `Tab { id, label, kind, split, panes }`
- `Pane { id, kind: shell|claude|codex, sessionId?, previousIds, accountId?, prompt?, command?, codexMode?: tui|appServer }`

**Not in the model (per device or per process):** the selected workspace, the active tab, focus, split ratios, a Claude pane's Terminal/Chat display, chat drafts, "Take control", and runtime state (terminal id, status, title, busy, waiting). A Codex pane's mode _is_ in the model, because it picks the process.

## Commands (`command.rs`)

The wire shape is `{"type": "<camelCase name>", ...camelCase fields}`.

| Command                                                                                                                  | Rule                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `openWorkspace{projectPath, projectName, worktreePath?, branch?, renderer}`                                              | Reuses the workspace with the same project and worktree. A worktree path equal to the project means the main checkout.                                                                                                                                                                                                                                                                                                                                                      |
| `closeWorkspace{workspaceId}` / `closeProject{projectPath}`                                                              | Ends every pane.                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| `newSession{workspaceId \| projectPath+worktreePath?, kind, resume?, prompt?, accountId?, label?, command?, codexMode?}` | By path: the exact main or worktree workspace, otherwise a new transient one (never the main workspace for a worktree cwd). A `resume` of a session a pane already runs (by id or `previousIds`, same kind) returns that pane with no spawn. Claude gets its session id up front. A resumed session drops `prompt`. Labels are `Claude N` / `Codex N` (count of that kind + 1) or `Terminal N` (count of all tabs + 1). `command` is shell only; `codexMode` is Codex only. |
| `closePane{paneId}` / `closeTab{tabId}`                                                                                  | Each closed pane is an End. A tab goes with its last pane, and a split holding it is dropped.                                                                                                                                                                                                                                                                                                                                                                               |
| `restart{tabId}`                                                                                                         | AI tabs only. Stop, then spawn on the same pane ids, session, account and prompt. A Claude pane with no id gets a new one.                                                                                                                                                                                                                                                                                                                                                  |
| `setCodexMode{paneId, mode}`                                                                                             | Stop, then spawn the other process on the same thread.                                                                                                                                                                                                                                                                                                                                                                                                                      |
| `rename{tabId, label}`                                                                                                   | Trimmed; an empty label is refused.                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| `split{tabId, direction}`                                                                                                | Pairs the tab with the next tab, else the previous one, else a new shell tab. The same direction again unsplits; another direction turns the split. Refused in a native workspace.                                                                                                                                                                                                                                                                                          |
| `movePane{paneId, tabId}` / `moveTab{tabId, toTabId}` / `moveWorkspace{...}`                                             | Within one workspace, and into a tab of the same kind. A tab emptied by a move goes.                                                                                                                                                                                                                                                                                                                                                                                        |
| `trustFolder{paneId}`                                                                                                    | Claude panes only.                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `sessionAttached{paneId, sessionId}`                                                                                     | Server fold-in: a Codex thread got its id, or `/resume` switched conversation (clears `previousIds`).                                                                                                                                                                                                                                                                                                                                                                       |
| `sessionRekeyed{sessionId, previousIds}`                                                                                 | Server fold-in for `/clear`: the pane on any of `previousIds` follows to the new id and keeps the old ones.                                                                                                                                                                                                                                                                                                                                                                 |
| `paneExited{paneId}`                                                                                                     | Server fold-in: no change. The pane stays, so it can be restarted.                                                                                                                                                                                                                                                                                                                                                                                                          |

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

- One pane per live session: a resume never starts a second process on a session a pane holds.
- `stop`/`end` come before the `spawn` that replaces them.
- Pane ids survive restart, mode switch, re-key and moves. Clients key local state on them.
- Every mutation ends with `persist`; a no-op returns no effects.

## File (`persist.rs`)

`workspaces.json` is `{version: 2, workspaces, local}`. `local` holds the desktop's per-device state (`selectedId`, `activeTabIds`, `chatPanes`, `serverTerminalIds`) until the desktop stores it itself. A file without `version` is the old desktop snapshot, and `load` migrates it:

- Claude `view: "chat"` goes to `local.chatPanes`.
- Codex `view` becomes `codexMode`.
- A Codex launch command is reduced to its prompt argument.
- A Claude pane without an id gets one.
- An empty tab gets a shell pane.
- A split naming a missing tab is dropped.

A newer `version` is refused. Writes go through `paths::atomic_write`.
