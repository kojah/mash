// Sample data from the dashboard mockup, used until `mash dash` serves the API.
import type { AgentSummary, ChatSummary, Machine, SearchHit, Transcript } from "./types";

const chats: readonly ChatSummary[] = [
  {
    id: "compress",
    title: "Compress large message text in the history database",
    agent: "claude-code",
    machine: "xenia",
    project: "mash",
    updatedAt: "12m",
    messageCount: 64,
  },
  {
    id: "watcher",
    title: "Flaky watcher test on macOS",
    agent: "codex",
    machine: "atlas",
    project: "mash",
    updatedAt: "1h",
    messageCount: 22,
  },
  {
    id: "backfill",
    title: "Unique names for automatic backfills",
    agent: "claude-code",
    machine: "xenia",
    project: "mash",
    updatedAt: "2h",
    messageCount: 86,
  },
  {
    id: "views",
    title: "Rename gold artifacts to views",
    agent: "opencode",
    machine: "xenia",
    project: "mash",
    updatedAt: "Yesterday",
    messageCount: 47,
  },
  {
    id: "init",
    title: "Tidy the init setup messages",
    agent: "cursor",
    machine: "atlas",
    project: "mash",
    updatedAt: "2d",
    messageCount: 19,
  },
  {
    id: "retry",
    title: "Why the publish retry was rejected",
    agent: "codex",
    machine: "atlas",
    project: "mash",
    updatedAt: "Yesterday",
    messageCount: 31,
  },
  {
    id: "resume",
    title: "Resumable backfill progress",
    agent: "claude-code",
    machine: "xenia",
    project: "gohawk",
    updatedAt: "Sun",
    messageCount: 58,
  },
];

const byId = (id: string): ChatSummary => {
  const chat = chats.find((candidate) => candidate.id === id);
  if (chat === undefined) throw new Error(`unknown sample chat ${id}`);
  return chat;
};

const machines: readonly Machine[] = [
  { name: "xenia", chats: 874, lastIndexed: "2m", needsAttention: false },
  { name: "atlas", chats: 346, lastIndexed: "8m", needsAttention: false },
];

const agents: readonly AgentSummary[] = [
  { agent: "claude-code", chats: 842, lastChat: "12m", quiet: false },
  { agent: "codex", chats: 214, lastChat: "1h", quiet: false },
  { agent: "opencode", chats: 96, lastChat: "Yesterday", quiet: false },
  { agent: "cursor", chats: 68, lastChat: "2d", quiet: false },
  { agent: "copilot", chats: 0, lastChat: "19d", quiet: true },
];

const hits: readonly SearchHit[] = [
  {
    chat: byId("backfill"),
    messageId: "backfill-3",
    before: "…so each automatic ",
    match: "backfill",
    after:
      " gets a fresh name, auto-<revision>, and a retry is never rejected as a reused identity…",
  },
  {
    chat: byId("retry"),
    messageId: "retry-2",
    before: "…the ",
    match: "backfill",
    after: " identity was reused and rejected…",
  },
  {
    chat: byId("resume"),
    messageId: "resume-2",
    before: "…resumable ",
    match: "backfill",
    after: " with lanes, renumbered after the head…",
  },
];

const transcripts: Readonly<Record<string, Transcript>> = {
  watcher: {
    chat: byId("watcher"),
    messages: [
      {
        id: "watcher-1",
        role: "user",
        at: "Today 09:58",
        blocks: [
          {
            type: "text",
            markdown:
              "The watcher test fails about one run in five on macOS. Can you find out why and fix it?",
          },
        ],
      },
      {
        id: "watcher-2",
        role: "agent",
        at: "Today 09:59",
        blocks: [
          {
            type: "tools",
            calls: [
              {
                kind: "read",
                verb: "Read",
                target: "app/watch.rs",
                status: "312 lines",
                tone: "plain",
                output: {
                  lines: [
                    "fn wait_quiet(&mut self, quiet: Duration) {",
                    "    let deadline = Instant::now() + quiet;",
                  ],
                  omitted: 0,
                },
              },
              {
                kind: "run",
                verb: "Ran",
                target: "cargo test watch_flow -- --test-threads 1",
                status: "Exit 101 · 4.1 s",
                tone: "bad",
                cwd: "~/scratch/mash",
                output: {
                  lines: [
                    "test watch_flow::watcher_coalesces_writes ... FAILED",
                    "assertion `left == right` failed",
                    "  left: 2",
                    "  right: 1",
                  ],
                  omitted: 212,
                },
              },
              {
                kind: "edit",
                verb: "Edited",
                target: "app/tests/watch_flow.rs",
                status: "+3 −3",
                tone: "ok",
                diff: [
                  "@@ watch_flow.rs:88 @@",
                  "-    sleep(Duration::from_millis(50));",
                  "+    watcher.wait_quiet(Duration::from_millis(250));",
                ],
              },
              {
                kind: "web",
                verb: "Fetched",
                target: "developer.apple.com/documentation/coreservices/file_system_events",
                status: "Not recorded",
                tone: "plain",
                missing: "The agent did not save this tool’s output, so only the request is shown.",
              },
            ],
          },
          {
            type: "text",
            markdown:
              "The test asserted 50 ms after the write, but FSEvents coalesces events for longer, so the second write sometimes landed in the same batch. It now waits for a quiet period instead.",
          },
          {
            type: "code",
            language: "rust",
            file: "app/tests/watch_flow.rs",
            code: 'watcher.wait_quiet(Duration::from_millis(250));\nassert_eq!(watcher.next()["changed_messages"], 1);',
          },
        ],
      },
    ],
  },
};

export const sample = { chats, machines, agents, hits, transcripts };
