# Codex integration

Workbench supports Codex terminal and native chat sessions on desktop and mobile. The current integration targets the **0.160.0** app-server protocol, retaining the recorded **0.159.3** conversation fixture for regression coverage. Exact schemas were generated from the installed CLI, including its experimental bindings; this does not assert that 0.160.0 is the latest release.

The implementation also integrates the shared attachments, elicitation UI, and mobile notification improvements that landed on main during development.

The original [integration audit](CODEX_AUDIT_2026-10-04.md) describes the unmodified `origin/main` baseline. This document describes the implementation that followed it.

## Chat controls

Expand **Codex controls** above the composer to access the following actions:

- **Actions:** compact context; fork into a separate chat; native review of uncommitted changes, a base branch, a commit, or custom instructions; rename and archive the thread. Collaboration modes such as Plan are offered when advertised by Codex, independently of approval/sandbox presets. Speed choices come from the selected model's service tiers.
- **Queue:** choose Steer to update the running turn or Queue to wait for it to finish. Edit, move, remove, or explicitly send a queued message. Interrupt pauses automatic delivery; Resume queue starts it again. The queue belongs to the running Workbench session, is shared by connected clients, and is not persisted across process shutdown. Failed queued turn starts retain the message and pause delivery.
- **Threads:** search the CLI's provider-backed history for the current folder, browse more results, inspect archived conversations, and unarchive them. Forking detaches the new thread from the original app-server before opening it in another pane, preserving the original session's identity. Load earlier messages pages history independently for each connected device; the normal snapshot stays bounded.
- **Goal:** read, set, pause, resume, or clear a durable objective, with an optional positive token budget and reported progress. Codex owns goal continuation.
- **Setup:** inspect account and usage, MCP servers, plugins, hooks, thread attachments, and background terminals. Start/cancel a native ChatGPT login or initiate MCP OAuth. External URLs open only after a click. Stop an individual background process or explicitly stop all background processes.
- **Remote:** inspect native remote status, explicitly enable ephemeral access, request a pairing code, list clients, revoke a client, or disable access. This is Codex's experimental remote service, separate from Workbench's bearer-authenticated phone connection.
- **Voice:** inspect available voices and explicitly start/stop a microphone conversation using the app-server realtime transport. The client sends bounded PCM16 chunks and plays output audio; closing its pane releases the microphone and stops the realtime session it started. macOS and Android include microphone declarations. WebView, CLI, and account support are required.

Text files can be selected for the next prompt, removed before submission, and optionally saved as a thread attachment. Picking or saving a file does not submit a model prompt. Inline text supplies model context; the durable `workbench/file` attachment is application-owned metadata. Text intake is limited to ten files of 64 KB each.

The server forwards a typed allowlist of actions, supplies the owned thread/folder, validates parameters, and correlates results to the requesting client. It does not expose arbitrary JSON-RPC forwarding. Optional read probes discover capabilities without initiating login, pairing, goals, voice, or paid inference. Missing optional APIs leave the chat usable and return action errors. Experimental negotiation permits optional methods; it does not guarantee that an account can use them.

## Settings, approvals, and transcript fidelity

Chat preserves approval policy and sandbox settings independently, including partial overrides and pairs that do not match a friendly preset. Effective settings and reasoning effort are hydrated from thread responses. Model catalogs page across results and retain default/supported effort, input modalities, and service tiers. Switching to an incompatible model explicitly resets sticky reasoning effort and speed overrides. Image prompts are refused locally when the advertised model explicitly lacks image support. Usage limits are read at startup and refreshed from notifications.

Network approvals show the destination and requested scope. Available decisions are checked before an approval is consumed; session permission is offered only when the CLI offers it, never as a persistent exec-policy amendment. Questions preserve IDs, secret/free-text flags, and blocking semantics. Duplicate question text remains distinguishable, secret answers are redacted from transcript state, and optional questions do not mark the session as waiting on the user.

MCP elicitation uses the shared Claude/Codex form and URL UI. Codex answers are correlated, retryable, and validated server-side, including required fields, enumerated choices, numeric constraints, and string validation. Form validation runs on the server before consuming the request. Unsupported forms can be declined or cancelled; advanced-form capability is not advertised. Unsupported host requests receive an immediate protocol reply so the CLI cannot wait indefinitely.

The shared chat renders subagent lifecycle/status with child-thread output, native review notices, configuration warnings, hook/MCP progress, turn diffs, generated images, and richer MCP text/resource/structured results. Image artifacts are fetched by known tool ID through the authenticated chat connection. Unknown item kinds produce a visible fallback and diagnostic rather than disappearing silently. The [protocol inventory](../crates/workbench-core/src/codex_transcript/fixtures/protocol-0.160.0.json) classifies notification, request, and item unions; [the checker](../scripts/check-codex-protocol.py) runs in frontend CI and can compare regenerated bindings with `--bindings`.

## Configuration and ownership

Core Codex paths honor `CODEX_HOME`, falling back to `~/.codex`. The integration installer edits root TOML keys structurally while retaining comments, existing fallback filenames, unrelated settings, and the previous notify command. The bridge passes the original payload to the previous command with literal argument quoting. Invalid TOML or unsupported notify values are preserved and reported as errors. Config and notify-command backups use atomic writes.

Workbench owns one private stdio app-server per chat session. Supported terminal launches add `--no-daemon`, discovered through a bounded CLI help probe; older CLIs retain their supported invocation. Workbench does not attach to or shut down a user's shared Codex daemon. Native remote access uses explicit ephemeral enablement on the owned process. This ownership choice avoids making pane cleanup responsible for unrelated daemon sessions; a future shared-daemon implementation needs explicit attachment and detach lifecycle tests.

Process IO uses one ordered writer with bounded queued bytes/count; the transcript lock is released before writing to the pipe. Pending native RPCs expire after 30 seconds. CLI stdout lines are capped at 16 MB; stderr lines at 64 KB. Retained transcript items, streamed text, full tool output, images, history display, and follow-up queues have separate limits. Settled transcript pruning preserves live approvals and reindexes retained items. The changes apply to the shared process writer used by Claude as well, covered by the workspace suites.

## Validation and remaining limits

Validation includes the Rust workspace suites, all JavaScript suites, Svelte/TypeScript checks, lint/format checks, the protocol comparison against generated 0.160.0 bindings, real HTTP transport integration, and desktop production/smoke builds. Fake app-server integration exercises explicit actions alongside startup, streaming, approvals, resume, and shutdown; helper/store tests cover queue recovery, independent history cursors, secret questions, form validation, and microphone acquisition after disposal.

An isolated real CLI probe used a temporary `CODEX_HOME`, initialized experimental capabilities, read the model/collaboration/remote-status APIs, and created an empty read-only thread. Its owned process exited cleanly. No paid inference, account login, native remote pairing, or live voice conversation was performed. Those paths are implemented and schema-checked, but require an authenticated CLI/account/device to validate end to end. Availability errors remain visible in the controls.

The official [app-server guide](https://learn.chatgpt.com/docs/app-server), [steering and queuing guide](https://learn.chatgpt.com/docs/prompting#steering-and-queuing), and [remote connection guide](https://learn.chatgpt.com/docs/remote-connections) explain the protocol and product semantics. For exact wire shapes, regenerate bindings from the supported CLI version before changing the implementation.
