// Answers the dashboard's requests in the browser, in the shapes `mash dash`
// sends, from the made-up history. The dashboard's own parsers read every
// answer, so a shape that drifts from the real one fails the tests here.
import type { Transport } from "../lib/transport";
import { chats, type DemoChat, type DemoMessage, localMachine, records } from "./history";
import tools from "./tools.json";

const DAY = 86_400_000;
const MINUTE = 60_000;

export interface Answer {
  readonly status: number;
  readonly body: unknown;
}
const ok = (body: unknown): Answer => ({ status: 200, body });
const refuse = (status: number, message: string): Answer => ({
  status,
  body: { error: { message } },
});
const notInDemo = (what: string): Answer =>
  refuse(403, `${what} is switched off in the demo. Install Mash to use it on your own history.`);

function updatedAt(chat: DemoChat, now: number): number {
  return Math.round(now - chat.daysAgo * DAY);
}
function createdAt(chat: DemoChat, now: number): number {
  return updatedAt(chat, now) - chat.messages.length * 2 * MINUTE;
}
function row(chat: DemoChat, now: number) {
  return {
    chat_id: chat.id,
    provider: chat.agent,
    node_id: chat.machine,
    title: chat.title,
    project: chat.project,
    repository: chat.repository,
    created_at: createdAt(chat, now),
    updated_at: updatedAt(chat, now),
  };
}
const messageId = (chat: DemoChat, position: number): string => `${chat.id}-${position}`;
const callId = (chat: DemoChat, position: number): string => `${chat.id}-call-${position}`;
function message(chat: DemoChat, entry: DemoMessage, position: number, now: number) {
  return {
    id: messageId(chat, position),
    ordinal: position,
    role: entry.role,
    content: entry.content,
    created_at: createdAt(chat, now) + position * 2 * MINUTE,
    ...(entry.call
      ? { tools: { tool_calls: [{ ...entry.call, call_id: callId(chat, position) }] } }
      : {}),
    ...(entry.answers === undefined
      ? {}
      : { tools: { tool_results: [{ call_id: callId(chat, entry.answers), is_error: false }] } }),
  };
}
function messages(chat: DemoChat, now: number) {
  return chat.messages.map((entry, position) => message(chat, entry, position, now));
}

const IGNORED = new Set([
  "the", "and", "for", "did", "does", "was", "were", "what", "when", "where", "which", "who",
  "why", "how", "our", "about", "with", "that", "this", "have", "has", "had", "are", "you",
  "your", "from", "into", "last", "week", "should", "can", "could",
]); // biome-ignore format: a word list reads best packed.

/** The words of a question worth matching, without common endings. */
export function terms(query: string): string[] {
  return [
    ...new Set(
      query
        .toLowerCase()
        .split(/[^a-z0-9]+/)
        .filter((word) => word.length > 2 && !IGNORED.has(word))
        .map((word) => word.replace(/(ing|ed|es|s)$/, "")),
    ),
  ].filter((word) => word.length > 2);
}
function snippet(text: string, wanted: readonly string[]): string {
  const plain = text.replace(/\s+/g, " ").trim();
  const lower = plain.toLowerCase();
  const at = Math.min(...wanted.map((term) => lower.indexOf(term)).filter((index) => index >= 0));
  const start = Math.max(0, at - 70);
  const piece = plain.slice(start, start + 220);
  return `${start > 0 ? "…" : ""}${piece}${start + 220 < plain.length ? "…" : ""}`;
}
function list(params: URLSearchParams, key: string): string[] {
  return (params.get(key) ?? "").split(",").filter(Boolean);
}

function search(params: URLSearchParams, now: number): Answer {
  const wanted = terms(params.get("q") ?? "");
  const agents = list(params, "provider");
  const machines = list(params, "machine");
  const projects = list(params, "project");
  const days = params.get("days");
  const since = days === null ? 0 : now - Number(days) * DAY;
  const results = chats
    .filter(
      (chat) =>
        (!agents.length || agents.includes(chat.agent)) &&
        (!machines.length || machines.includes(chat.machine)) &&
        (!projects.length || projects.includes(chat.project)) &&
        updatedAt(chat, now) >= since,
    )
    .flatMap((chat) => {
      // The best message is the one that holds the most of the question's words.
      const scored = chat.messages
        .map((entry, position) => {
          const text = entry.content.toLowerCase();
          return { entry, position, found: wanted.filter((term) => text.includes(term)).length };
        })
        .filter((candidate) => candidate.found > 0 && candidate.entry.role !== "tool")
        .sort((a, b) => b.found - a.found || a.position - b.position);
      const title = chat.title.toLowerCase();
      const inTitle = wanted.filter((term) => title.includes(term)).length;
      const best = scored[0];
      if (!best && !inTitle) return [];
      const shown = best ?? { entry: chat.messages[0], position: 0, found: 0 };
      if (!shown.entry) return [];
      return [
        {
          score: inTitle * 3 + (best?.found ?? 0) * 2 + scored.length - chat.daysAgo / 100,
          result: {
            ...row(chat, now),
            matches: [
              {
                message_id: messageId(chat, shown.position),
                node_id: chat.machine,
                role: shown.entry.role,
                snippet: best
                  ? snippet(shown.entry.content, wanted)
                  : shown.entry.content.slice(0, 220),
              },
            ],
          },
        },
      ];
    })
    .sort((a, b) => b.score - a.score)
    .map((scored) => scored.result);
  return ok({
    results,
    next_offset: null,
    notice:
      "The demo matches the words in your question. Mash itself also matches by meaning, so it finds chats that say the same thing differently.",
  });
}

function chatOf(id: string | null): DemoChat | undefined {
  return chats.find((chat) => chat.id === id);
}
function transcript(params: URLSearchParams, now: number): Answer {
  const chat = chatOf(params.get("id"));
  if (!chat) return refuse(404, "That chat is not in the demo's history.");
  const offset = Number(params.get("offset") ?? 0);
  const limit = Number(params.get("limit") ?? 100);
  return ok({ messages: messages(chat, now).slice(offset, offset + limit) });
}
function view(params: URLSearchParams, now: number): Answer {
  const id = params.get("id") ?? "";
  const chat = chats.find((candidate) => id.startsWith(`${candidate.id}-`));
  if (!chat) return refuse(404, "That message is not in the demo's history.");
  return ok({ messages: messages(chat, now) });
}

function record(entry: (typeof records)[number], now: number) {
  const chat = chatOf(entry.source.chat);
  const details =
    entry.kind === "decision"
      ? {
          kind: entry.kind,
          state: "accepted",
          alternatives: entry.alternatives ?? [],
          consequences: entry.consequences ?? [],
        }
      : entry.kind === "preference"
        ? { kind: entry.kind, state: "active", conditions: [] }
        : { kind: entry.kind, state: "current", reported_by: "Sam", valid_until: null };
  return {
    id: entry.id,
    owner: "sam",
    node_id: chat?.machine ?? localMachine,
    scope: entry.project ? { kind: "repository", identity: entry.project } : { kind: "personal" },
    current_revision: 1,
    conflicted: false,
    revisions: [
      {
        revision: 1,
        recorded_at: Math.round(now - entry.daysAgo * DAY),
        evidence_current: true,
        confirmations: 0,
        confirmation_history: [],
        body: {
          statement: entry.statement,
          rationale: entry.rationale,
          recorded_by: entry.recordedBy,
          effective_at: null,
          review: entry.review,
          details,
        },
        evidence: [
          {
            chat_id: entry.source.chat,
            message_id: `${entry.source.chat}-${entry.source.message}`,
            fingerprint: `demo-${entry.id}`,
            quote: entry.source.quote,
            purpose: "endorsement",
            source_role: "user",
          },
        ],
      },
    ],
    links: [],
    next_before_revision: null,
    next_link_offset: null,
  };
}
function recordList(params: URLSearchParams, now: number): Answer {
  const wanted = terms(params.get("q") ?? "");
  const kind = params.get("kind");
  const review = params.get("review");
  const chat = params.get("chat");
  const state = params.get("state");
  const current = ["accepted", "active", "current"];
  return ok({
    records: records
      .filter(
        (entry) =>
          (!kind || entry.kind === kind) &&
          (!review || entry.review === review) &&
          (!chat || entry.source.chat === chat) &&
          (!state || current.includes(state)) &&
          wanted.every((term) =>
            `${entry.statement} ${entry.rationale}`.toLowerCase().includes(term),
          ),
      )
      .map((entry) => record(entry, now)),
    next_offset: null,
  });
}

const roots: Readonly<Record<string, readonly string[]>> = {
  "claude-code": ["/home/sam/.claude/projects"],
  codex: ["/home/sam/.codex/sessions"],
  cursor: ["/home/sam/.config/Cursor/User/workspaceStorage"],
  copilot: ["/home/sam/.copilot/session-state"],
  opencode: ["/home/sam/.local/share/opencode"],
};
function status(): Answer {
  const local = chats.filter((chat) => chat.machine === localMachine);
  const count = local.reduce((sum, chat) => sum + chat.messages.length, 0);
  return ok({
    ready: true,
    chats: local.length,
    messages: count,
    searchable_messages: count,
    models_installed: true,
    realized_mode: "hybrid",
    database_path: "/home/sam/.local/share/mash/mash.sqlite",
    recommended_action: null,
    configuration: {
      path: "/home/sam/.config/mash/config.toml",
      loaded: true,
      providers: Object.fromEntries(
        Object.entries(roots).map(([name, paths]) => [name, { roots: paths }]),
      ),
      index: { since_days: 90, destination: "both" },
      embeddings: { backend: "local" },
      qdrant: { url: "http://127.0.0.1:6333", vector_search: { algorithm: "hnsw" } },
      views: { query: { ranking: "contextual-rerank" } },
      records: { enabled: true },
      redaction: { output: true, publication: true },
      shared: { group_id: "harbor", node_id: localMachine, api_url: "https://mash.harbor.example" },
    },
  });
}
function settings(): Answer {
  return ok({
    path: "/home/sam/.config/mash/config.toml",
    revision: "demo",
    editable: false,
    agents: Object.fromEntries(
      Object.entries(roots).map(([name, paths]) => [name, { indexed: true, roots: paths }]),
    ),
    since_days: 90,
    destination: "both",
    shared: true,
    memory_capture: true,
    hide_credentials: { output: true, publication: true },
  });
}
/** What a dashboard with no service management reports: this machine's index, nothing to control. */
function machine(now: number) {
  const checked = Math.round(now / 1000);
  return {
    id: localMachine,
    platform: "linux",
    checked_at: checked,
    reachable: true,
    warnings: [],
    services: [
      {
        id: "sqlite",
        name: "SQLite index",
        kind: "sqlite",
        state: "healthy",
        summary: "Readable · compatible",
        checked_at: checked,
        pid: null,
        actions: [],
        control_note: "SQLite is a file, not a process. Checks do not change or migrate it.",
        control_revision: null,
        diagnostics: [{ label: "Read check", value: "Opened read-only." }],
        compatibility: null,
        logs_available: false,
      },
    ],
  };
}

/** The answer `mash dash` would give to one request, from the demo's history. */
export function respond(url: string, method: string, now: number): Answer {
  const { pathname, searchParams } = new URL(url, "http://demo.invalid");
  if (method !== "GET") return notInDemo("Changing anything");
  switch (pathname) {
    case "/api/chats":
      return ok({
        local_node: localMachine,
        truncated: false,
        chats: chats.map((chat) => row(chat, now)),
      });
    case "/api/search":
      return search(searchParams, now);
    case "/api/chat":
      return transcript(searchParams, now);
    case "/api/view":
      return view(searchParams, now);
    case "/api/records":
      return recordList(searchParams, now);
    case "/api/record": {
      const entry = records.find((candidate) => candidate.id === searchParams.get("id"));
      return entry ? ok(record(entry, now)) : refuse(404, "That memory is not in the demo.");
    }
    case "/api/status":
      return status();
    case "/api/console/tools":
      return ok(tools);
    case "/api/console/call":
      return notInDemo("Running tools");
    case "/api/services/access":
      return ok({ authentication_required: false, local_node: localMachine });
    case "/api/services":
      return ok({ machines: [machine(now)] });
    case "/api/services/local":
      return ok(machine(now));
    case "/api/services/settings":
      return settings();
    case "/api/services/fleet":
      return ok({ enabled: false, revision: null, actions: [], services: [] });
    case "/api/services/operations":
      return ok([]);
    case "/api/services/installation":
      return ok({
        build: tools.build_revision,
        agents: (["claude-code", "codex"] as const).map((name) => ({
          agent: name,
          expected: true,
          connected: true,
          steps: (["plugin", "skill", "mcp"] as const).map((component) => ({
            component,
            state: "present",
            message: "Installed and readable.",
            remediation: null,
          })),
        })),
      });
    default:
      return refuse(404, "This part of Mash is not in the demo.");
  }
}

/** Installed by the demo's entry point in place of requests to `mash dash`. */
export const transport: Transport = (path, init) => {
  const answer = respond(path, init.method ?? "GET", Date.now());
  return Promise.resolve(
    new Response(JSON.stringify(answer.body), {
      status: answer.status,
      headers: { "Content-Type": "application/json" },
    }),
  );
};
