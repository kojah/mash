# Transcript rendering model

Status: proposed, not implemented. Today `/api/chat` returns silver messages and
the dashboard infers a presentation from `role`, `metadata` and a `[Thinking]`
prefix. The structured tool rows in `ToolCard.svelte` only receive sample data.

This document specifies one rendering model for every provider, so the transcript
does not need to know how a particular agent stored its history.

## Goals

- Draw every provider's chat from one typed event list.
- Derive events when a chat is read. Silver stays canonical and unchanged: no
  schema migration, reindex or view rebuild.
- Never lose content. Anything unrecognized renders as plain text or a generic
  row, and "Not recorded" means the provider did not keep that field.
- Share the projection between the dashboard and transcript exports.

Non-goals: changing search or embedding text, merging chats across providers,
and editing history.

## What silver holds

Common shape: `messages(role, content, metadata, attachments, context_parent)`.
`metadata` is `crate::tools::Activity`: `tool_calls[{name?, call_id?, input?}]`
and `tool_results[{call_id?, is_error?}]`. Result output stays in `content`.

Evidence for Claude Code and Codex comes from local silver on Xenia on
2026-11-09 (697 and 431 chats on the shared index). The other providers had no
local data, so their rows come from importer source and need real samples before
implementation.

| Provider | Tool calls | Tool results | Thinking | Branch links |
|---|---|---|---|---|
| Claude Code | `assistant` with named `tool_calls` and structured `input` | Separate `user` rows with `tool_results`, linked by `call_id`, with `is_error` | `[Thinking] ` prefix | `context_parent` on every message |
| Codex | Mostly typed JSON in `assistant.content`; `metadata` has only `call_id`. Some named calls in `metadata` | Inside the same JSON, or `tool` rows linked by `call_id` | Not observed in the sampled chat | None |
| OpenCode | Text: `Tool <name>: <state>` | None | `Reasoning: <text>` | Linear |
| Copilot | Text: `Tool: <name>` then arguments | None confirmed | `Reasoning: <text>`, possibly mid-message | Session events have parents; chat history is linear |
| Cursor | Text: `Tool: <name>` then arguments | Text: `Tool result: <name>` then result | `Thinking: <text>`, `Thinking: [redacted]` | Linear |
| Antigravity | No structured activity | None | To confirm | Linear |

Codex `content.type` values observed: `commandExecution` (`command`, `cwd`,
`exitCode`, `durationMs`, `status`, `aggregatedOutput`), `fileChange` (`changes`,
`status`), `mcpToolCall` (`server`, `tool`, `arguments`, `result`, `error`,
`status`, `durationMs`), `webSearch` (`action`, `query`, `results`), `imageView`
(`path`), `subAgentActivity` (`agentThreadId`, `agentPath`, `kind`),
`contextCompaction`, and `sleep` (`durationMs`). The last four have no `metadata`.
One sampled message body reached 1.7 MB.

Claude Code `user` rows also carry injected content that the person did not type,
including `<system-reminder>`, `<task-notification>`, `<local-command-caveat>`,
`<artifact-view-context>` and `<command-name>`.

## Event model

Rust types in the shared core, with enums for known alternatives. Generic JSON is
limited to opaque tool payloads.

```rust
enum TranscriptEvent {
    Message { id, author: Author, markdown, attachments },
    Thinking { id, text, redacted: bool },
    Tool(ToolEvent),
    Delegation { id, label, thread: Option<ChatRef>, children: Vec<TranscriptEvent> },
    Notice { id, kind: NoticeKind, text: Option<String> },
    Unknown { id, role, text },
}

enum Author { You, Agent }

struct ToolEvent {
    id,
    call_id: Option<String>,
    kind: ToolKind,          // Read, Search, Edit, Run, Web, Agent, Other
    label: String,           // Display name, such as "Bash" or "read_doc"
    server: Option<String>,  // MCP server, when known
    target: Option<String>,  // Path, command, query or URL
    status: ToolStatus,      // Ok, Failed, Denied, Running, Unknown
    exit_code: Option<i32>,
    duration_ms: Option<u64>,
    cwd: Option<String>,
    input: Option<serde_json::Value>,
    output: Option<OutputPreview>,
    diff: Option<Vec<DiffLine>>,
    missing: Option<MissingReason>,
}

struct OutputPreview { tail: String, omitted_lines: u64, omitted_bytes: u64 }

enum NoticeKind {
    ContextCompacted, SystemContext, Command, Wait, Interrupted, ApiError,
    TimeGap, HistoryUnavailable,
}
```

Every event carries the source message ID, so links to a message and search hits
keep working. One source message can yield several events.

## Projection rules

Dispatch on the `Provider` enum to concrete functions, without provider traits or
registries. Apply the first matching rule. A failed parse falls through to
`Message`, or `Unknown` for non-dialogue roles.

### Claude Code

1. `[Thinking] ` prefix: `Thinking`.
2. `tool_calls`: one `ToolEvent` per call, plus a `Message` for any assistant text.
3. `tool_results`: attach `content` and `is_error` to the call with that `call_id`.
   Never treat these rows as the person's messages. An unmatched result becomes a
   `ToolEvent` with `kind: Other`.
4. A `user` row with no `tool_results` that starts with an allowlisted injected tag:
   `Notice`. `<command-name>` becomes `Command`.
5. Otherwise `Message`.

Targets come from known input fields: `Bash.command`, `Read`, `Edit` and
`Write.file_path`, `Grep.pattern`, `WebFetch.url`, `WebSearch.query`,
`Agent.description`. `Edit.old_string` and `new_string` produce the diff.

### Codex

1. An `assistant` row whose content is a JSON object with a known `type` maps as
   follows. An unknown `type` becomes a generic `ToolEvent` with the raw body.
   Require a known `type`, so an ordinary reply containing JSON stays a `Message`.

   | `type` | Event |
   |---|---|
   | `commandExecution` | `Tool(Run)` with command, cwd, exit code, duration and output |
   | `fileChange` | `Tool(Edit)` with diff from `changes` |
   | `mcpToolCall` | `Tool(Other)` with server, tool, arguments, result and error |
   | `webSearch` | `Tool(Web)` with query |
   | `imageView` | `Tool(Read)` with path |
   | `subAgentActivity` | `Delegation` |
   | `contextCompaction` | `Notice(ContextCompacted)` |
   | `sleep` | `Notice(Wait)` |

2. Named `tool_calls`: a `ToolEvent` from `metadata`.
3. `tool` rows: attach to the call by `call_id`.
4. `system` and `developer`: `Notice(SystemContext)`.
5. Otherwise `Message`.

### OpenCode, Copilot, Cursor and Antigravity

These importers flatten activity into text, so the projection parses only exact
line-start forms from the table above, and only on assistant rows. Status and
output are unavailable unless recorded, so `missing` is set. A person typing
`Tool: x` must stay a `Message`.

Text parsing is a stopgap. The durable fix is for those importers to emit
`Activity`, as a separate change that re-ingests those providers.

### Shared rules

- **Tool kinds:** map known names per provider. Anything else is `Other`, and it
  still renders with its name and raw input.
- **MCP names:** split `mcp__server__tool` and `server.tool` into `server` and
  `label`. Opaque connector IDs are not shown as the server name.
- **Status:** `is_error` or a non-zero exit is `Failed`. A call with no result at
  the end of a chat is `Unknown`, and it is `Running` only with a live status.
- **Branches:** show the switcher only where a parent has more than one child.
  `context_parent` alone does not mean a fork.

## Reading and limits

- Run the projection on a blocking worker, in the same core operation that serves
  chat messages.
- Paging stays by message ordinal. When a page holds calls whose results fall on a
  later page, fetch those results by `call_id` so a row is never split.
- Send a bounded output tail with omitted counts. Load full output on request by
  message ID. Message size must not decide the transcript's response size.
- Shared chats use the same projection, because shared silver has the same schema.

## Rendering

The rendering targets are the Terminal mockup boards.

- Render all chat content as text or through the existing sanitizer. Archived
  history is untrusted.
- Remote images are click-to-load and never fetched automatically. Local
  attachments render inline, and missing ones say so.
- Consecutive tool events group together. `Notice` and `Thinking` start collapsed
  where they would otherwise interrupt reading.
- Exports use the same events, so a transcript and its export agree.

## Verification

- Per-provider fixtures from real, redacted samples, including malformed bodies
  and unknown `type` values.
- Proptest invariants: every source message contributes an event, order follows
  `ordinal`, each result attaches to at most one call, and arbitrary content never
  panics or disappears.
- Regressions for the known traps: Claude tool results as `user` rows, tool output
  starting with `<`, and an assistant reply that is valid JSON.
- Browser tests comparing each scenario with its mockup board at 1280 and 390 px.

## Rollout

1. Add the event types and the Claude Code and Codex projections behind `/api/chat`.
2. Move the transcript and exports to events, and remove role-based inference.
3. Collect samples for the other providers, then add their text projections.
4. Improve those importers to emit `Activity`.

## Open questions

- Does Codex record reasoning anywhere in silver? It was absent from the sampled
  chat, which is not enough to rule it out.
- Which subagent records can be linked to a child chat?
- Should the MCP `view` tool also expose events, or stay message-based?
