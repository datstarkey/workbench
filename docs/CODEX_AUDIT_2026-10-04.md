# Codex integration audit — 2026-10-04

This is the historical research baseline. See [Codex implementation](CODEX_IMPLEMENTATION.md) for the delivered changes and validation limits.

Fix permission-setting parity, TOML installation, and approval presentation first. The strongest next features are subagent visibility, explicit Queue/Steer controls, native review/compact/fork actions, and Plan mode. Native Codex remote control is now worth a separate architecture experiment.

Research baseline: freshly fetched `origin/main`, commit `014df0e05d6cae0e0a0e3708e446a2191c792a65`. Worktree: `/Users/jake/Repos/workbench-codex-research`; branch: `research/codex-capabilities-2026-10-04`. Workbench's recorded protocol fixture is Codex **0.159.3**; the installed CLI is **0.160.0**. This is a versioned comparison, not a claim that 0.160.0 is the newest published release.

Evidence combines repository inspection, freshly generated default and experimental 0.160.0 TypeScript bindings, official OpenAI documentation, existing tests, and synthetic probes against the actual core source. Findings marked “confirmed” were exercised locally; “inspection” means the code establishes the gap; “hypothesis” needs a real app-server scenario. No authenticated inference turn or native remote pairing was performed.

**Already implemented**

Workbench has new/resumed Codex chat on desktop and mobile, paginated resume hydration, terminal/chat handoff, streaming text/reasoning/command output, file patches, MCP tool text output, images as prompt inputs, model/reasoning selection, three permission presets, command/file/permission approvals, user questions, interruption, retry notices, usage/context displays, and enabled skill discovery/invocation with live refresh. Desktop and mobile also have history pickers; mobile already has Git changes review. These are not missing features.

See the [Codex driver](../apps/server/src/agent/codex.rs), [transcript](../crates/workbench-core/src/codex_transcript.rs), [shared chat store](../packages/chat-ui/src/agent-chat.svelte.ts), and [mobile documentation](MOBILE.md).

**Implementation improvements, in priority order**

1. **P1 — Preserve every selected approval/sandbox setting in chat. Confirmed.**

   [codexChatMode](../apps/desktop/src/lib/utils/claude.ts) recognizes only three complete pairs. `SessionChat` consequently sends no override for `never + read-only`, `never + workspace-write`, or a single non-default setting. Terminal command builders honor those selections. The existing test explicitly expects this fallback, so it is a current design limitation rather than an uncovered regression.

   Probe: `never + read-only` produces `codex -c tui.alternate_screen=never -c approval_policy=never -c sandbox_mode=read-only` for terminal launch, but `chatMode: null`. Chat then uses the CLI configuration, which may differ from the user's selected restrictions.

   Add independent optional policy/sandbox fields to the start contract, validate them server-side, and derive the friendly preset from the effective result. Omit only settings individually set to `default`. Keep the three presets as shortcuts. Test partial and non-preset combinations through desktop helper → HTTP body → driver RPC.

2. **P1 — Edit Codex config structurally. Confirmed for notify; inspection for fallback installation.**

   [ensure_codex_notify_config](../crates/workbench-core/src/codex_config.rs) appends the notify key to the end of the file. Given an existing TOML table, the key belongs to that table rather than the root:

   ```toml
   [model_providers.example]
   name = "Example"
   notify = ["bash", "/tmp/workbench-codex-notify.sh"]
   ```

   A probe called the actual function, then parsed its result with Python `tomllib`: root `notify` was absent; `model_providers.example.notify` was present. `ensure_codex_config` appends `project_doc_fallback_filenames` similarly. String-presence checks can also mistake a comment or nested key for successful installation. Replacing an existing notify command discards the user's previous integration.

   Use a formatting-preserving TOML editor to update root keys, verify effective values, and retain the existing atomic write. Preserve an existing notify integration through an explicit wrapper or clearly supported coexistence mechanism. Cover tables, comments, whitespace variants, existing fallback arrays, and idempotence. Run these tests on temporary files, never the user's config.

3. **P1 — Give network approvals their actual destination and scope. Confirmed.**

   The [approval adapter](../crates/workbench-core/src/codex_transcript/approvals.rs) keeps only `command` and `cwd` for command approvals. A request with `networkApprovalContext: { host: "example.com", protocol: "https" }` and no command becomes a `Bash` approval with two empty strings. The [approval card](../packages/chat-ui/src/ChatApproval.svelte) therefore cannot explain which destination is being approved.

   Carry the approval kind, network destination, environment, and relevant requested access into typed shared data. Render separate command/network/permissions presentations with explicit scope. Existing `item/permissions/requestApproval` handling already grants or denies the requested profile and supports session scope; improve its presentation and optionally allow granting a subset. Preserve available decisions when supplied, and keep lasting policy amendments distinct from “Allow for this session.” Experimental per-command permission fields need capability gating before relying on them.

4. **P1 — Preserve question semantics and identify answers by ID. Confirmed metadata loss; inspection of UI behavior.**

   The same adapter drops `isSecret`, `isOther`, and `isBlocking`; the [question component](../packages/chat-ui/src/ChatQuestion.svelte) always offers an ordinary visible-text Other field. A synthetic secret question with free text disabled and nonblocking behavior loses all three flags in the resulting transcript item. Answers are keyed by question text before being translated back to IDs, so repeated text cannot reliably distinguish questions.

   Preserve stable question IDs and flags, use secret-input presentation, respect free-text availability, and distinguish optional questions from approval blockers. Review whether secret answers should appear in transcript previews or persistence. Keep timeout handling compatible with the installed protocol: 0.160.0 marks `autoResolutionMs` deprecated in favor of `isBlocking`. Add adapter/store tests for duplicate text, secret questions, optional questions, withdrawal, and answers from another device.

5. **P1 — Surface subagents and stop silently dropping item kinds. Confirmed.**

   [apply_item](../crates/workbench-core/src/codex_transcript/items.rs) falls through silently for `collabAgentToolCall`, `subAgentActivity`, `enteredReviewMode`, `exitedReviewMode`, and `imageGeneration`. Probes show no changed items and no unknown-method diagnostic. These variants exist in the generated 0.160.0 `ThreadItem` union. Real activity can therefore disappear while the chat appears to be thinking.

   Start with subagent lifecycle/status cards and review entry/result notices. Adapt Codex child-thread information to the shared task model; [task_output](../apps/server/src/agent/session.rs) currently reads Claude-only task files, so Codex output needs its own backend. Add an informative fallback plus once-per-kind diagnostics for unrecognized items. Image generation also needs an artifact representation, rather than only a text-tool fallback. Replay representative nontrivial fixtures alongside the existing single-turn fixture.

6. **P2 — Hydrate model metadata completely and handle sticky overrides. Confirmed effort omission; inspection of pagination; hypothesis for incompatible sticky effort.**

   `apply_thread` discards the effective `reasoningEffort`; a response containing `high` leaves chat metadata's effort null. The driver's `Pending::Models` path consumes one page and ignores `nextCursor`. The picker can therefore omit catalog entries on a paginated server. Model options also discard default effort, input modalities, and service tiers.

   Load all catalog pages; expose effective/default effort and supported inputs; hydrate quota with `account/rateLimits/read` rather than waiting only for notifications. When a model switch invalidates the selected effort, send an explicit supported value. Currently `set_model` clears the local override and the next turn omits it, while the protocol makes earlier turn overrides sticky; reproduce whether a previous unsupported effort survives that model switch before calling it a runtime bug. Test this with two models having different effort capabilities. Respect provider/account capability failures rather than treating a catalog entry as guaranteed access.

7. **P2 — Add protocol compatibility checks, visible warnings, and bounded IO. Inspection.**

   Codex uses hand-written JSON without the exhaustive protocol inventory already present for Claude. [IGNORED](../crates/workbench-core/src/codex_transcript.rs) includes `warning`, `configWarning`, `deprecationNotice`, thread status/settings changes, turn diffs, hook activity, and MCP progress. Some are useful user information. Unknown item variants bypass even the existing unknown-method logging.

   Record a supported CLI baseline and classify generated request/notification/item unions in CI. Keep new optional features behind version/capability checks. Surface actionable configuration/auth/sandbox warnings. Add timeouts and recovery for pending RPCs after startup; the existing 30-second readiness wait should remain. Audit ordered writes across concurrent clients, failed writes after state mutation, and memory growth: streaming text appends and `BufRead::lines` are not bounded, while snapshot truncation limits transmission rather than all retained data. These are engineering improvements, not reproduced deadlocks or resource-exhaustion incidents.

**Missing feature candidates**

The method names and gating below come from the installed CLI's generated bindings. “Default bindings” means the generator emitted them without `--experimental`; it is not a blanket production-stability guarantee. Workbench currently initializes with `experimentalApi: false`.

| Priority | Feature                                                        | Implementation direction and current gap                                                                                                                                                                                                                                                                                                                                                             |
| -------- | -------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| P1       | Explicit Queue versus Steer                                    | Current `submit` steers a running turn; its temporary `queued` vector only waits for an in-flight start. Add visible, editable follow-ups that wait for completion. Native `thread/queue/*` methods are experimental; a Workbench-owned server queue is an alternative for older versions.                                                                                                           |
| P1       | Native review, compact, and fork actions                       | Add typed actions for `review/start`, `thread/compact/start`, and `thread/fork` from default bindings. Current unrecognized slash text goes to the model as ordinary input. Fork should create a separate pane/thread, preserving the original. Existing mobile Git diff review is a different feature.                                                                                              |
| P1       | Plan collaboration mode                                        | Use experimental `collaborationMode/list` and `turn/start.collaborationMode`. Codex's read-only permission preset is not a native Plan-mode selector. Keep collaboration mode separate from approval/sandbox controls.                                                                                                                                                                               |
| P1       | MCP elicitation                                                | `mcpServer/elicitation/request` currently receives an immediate decline without a UI item. Implement form and URL requests, validation, acceptance/decline/cancel, and withdrawal. Start with the declared schema variants; advanced forms require a separate initialization capability.                                                                                                             |
| P2       | Thread rename/archive and better history                       | `thread/name/set`, `thread/archive`, `thread/unarchive`, `thread/list`, and `thread/items/list` exist in default bindings. Replace or supplement the hard-coded JSONL scan with provider-backed listing, cwd filtering, titles, updated-time sorting, and load-older history. Keep a fallback for older CLIs. Current resume hydration stops at 500 protocol items with no client load-older action. |
| P2       | Durable goals                                                  | `thread/goal/set`, `/get`, and `/clear` exist in default bindings; goal notifications are explicitly ignored. Expose objective, user-controlled pause/resume/clear, budgets, and progress after verifying feature availability. Let Codex own continuation semantics.                                                                                                                                |
| P2       | Fast/service-tier selection                                    | Turn fields and model catalog service-tier metadata exist in default bindings. Offer only the server-advertised choices and show effective speed; test model changes and unavailable tiers.                                                                                                                                                                                                          |
| P2       | Account, MCP, plugin, and hook status                          | Query `account/read`, `mcpServerStatus/list`, `plugin/list`, and `hooks/list`; add native login/OAuth flows where useful. Current settings pages primarily manage Claude integrations; Codex skill invocation alone does not expose its whole setup. Respect custom Codex homes: core config/session paths currently always use `~/.codex`.                                                          |
| P2       | Native Codex remote control and daemon attachment              | Investigate experimental `remoteControl/*`, daemon/proxy transports, and CLI `remote-control`. Workbench already provides its own phone chat transport. Decide whether native pairing is an additional option and whether attaching a shared daemon improves terminal/chat continuity.                                                                                                               |
| P3       | File/context attachments, background terminals, richer outputs | Extend input beyond text/images/skills using the versioned `UserInput` and attachment schemas. Experimental background-terminal list/terminate/clean could expose long-running processes. Rich MCP content, generated images, and realtime voice need separate shared artifact/transport work.                                                                                                       |

Official documentation distinguishes queued follow-ups from steering and describes editing/reordering queued messages. That makes Queue/Steer a concrete UX improvement rather than merely renaming the startup buffer. [Prompting](https://learn.chatgpt.com/docs/prompting#steering-and-queuing).

**Remote control changes the architecture assumptions**

The repository's “Codex has no remote-control” statement is outdated for the installed CLI. `codex remote-control --help` lists `start`, `stop`, and `pair`; the generated experimental protocol includes enable/disable/status/pairing/client-revocation methods. Official documentation also describes this experimental command and the shared local daemon. [Developer commands](https://learn.chatgpt.com/docs/developer-commands).

Do a bounded experiment before integrating it: start/resume a thread, attach a second client, switch a pane between terminal/chat during a running turn, close one pane, then restart Workbench. Verify ownership and cleanup. Current terminal launch does not pass `--no-daemon`, while chat starts a private app-server; killing a terminal frontend may not stop work owned by an existing shared daemon. That is a compatibility hypothesis, not a reproduced duplicate-owner bug. A shared daemon must not be shut down merely because one Workbench pane closes.

Native pairing availability depends on host/account/workspace configuration. Do not equate defining the RPCs with successful access, or equate native Codex remote control with Workbench's existing bearer-authenticated control plane. [Remote connections](https://learn.chatgpt.com/docs/remote-connections).

**Suggested delivery sequence**

1. Fix independent permission overrides and structural config installation; cover both with meaningful regression tests.
2. Repair network approval and question metadata, then add unknown-item fallback/subagent cards.
3. Complete model/effort hydration, catalog pagination, warning handling, and compatibility fixtures.
4. Add native compact/review/fork controls and explicit Queue/Steer; extend the shared stores/protocol so desktop and mobile receive the same behavior.
5. Add experimental Plan and goals only with supported capability paths; run the daemon/native-remote experiment separately before choosing its ownership model.

Keep pure adaptation in `workbench-core`, process/RPC orchestration in the server driver, shared wire types in Rust plus `@workbench/types`, and UI logic in `@workbench/chat-ui`. Use helper/store tests and fake-app-server integration tests; retain the project's policy against component interaction unit tests.

**Verification and references**

Executed on the unmodified research baseline:

- `cargo test -p workbench-core codex --locked`: **50 passed**.
- `cargo test -p workbench-server codex --locked`: **3 driver tests and 1 fake-app-server HTTP/WS integration test passed**.
- Generated bindings with `codex app-server generate-ts --out …`, then again with `--experimental`. Compared `ClientRequest`, `ServerRequest`, `ServerNotification`, `ThreadItem`, start/resume responses, models, questions, and turn settings.
- Bun evaluated the actual desktop permission helper; a temporary Cargo probe used the actual core transcript and included the actual config source to call its private installer helper. Python `tomllib` verified the notify key's location. All probe inputs were synthetic; no user config was written.
- Open issues/open PRs were checked: no open issues; PR #148 concerns mobile session notifications, and PR #46 concerns embedded VS Code. Neither implements this backlog as of inspection.

The official [app-server guide](https://learn.chatgpt.com/docs/app-server) covers the integration protocol and experimental boundaries. Use generated bindings from the supported installed binary for exact field shapes; the documentation and local schema can differ. This research has not verified every candidate through a live inference session. Existing green tests validate the current implementation, including some current limitations; they do not establish completeness.
