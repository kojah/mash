// Answers the dashboard's requests in the browser, in the shapes `mash dash`
// sends, from the made-up history. The dashboard's own parsers read every
// answer, so a shape that drifts from the real one fails the tests here.
import type { Answer, ReadPath } from "../lib/routes";
import type { Transport } from "../lib/transport";
import { chats, type DemoChat, type DemoMessage, localMachine, records } from "./history";
import tools from "./tools.json";

const DAY = 86_400_000;
const MINUTE = 60_000;

/** What the demo sends back: a status and a JSON body. */
export interface Reply {
  readonly status: number;
  readonly body: unknown;
}
/** A request the demo turns down, with the reason shown to the reader. */
class Refusal {
  constructor(
    readonly status: number,
    readonly message: string,
  ) {}
}
const refuse = (status: number, message: string): Refusal => new Refusal(status, message);
const notInDemo = (what: string): Refusal =>
  refuse(403, `${what} is switched off in the demo. Install Mash to use it on your own history.`);

function updatedAt(chat: DemoChat, now: number): number {
  return Math.round(now - chat.daysAgo * DAY);
}
function createdAt(chat: DemoChat, now: number): number {
  return updatedAt(chat, now) - chat.messages.length * 2 * MINUTE;
}
function row(chat: DemoChat, now: number): Answer<"/api/chats">["chats"][number] {
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
function message(
  chat: DemoChat,
  entry: DemoMessage,
  position: number,
  now: number,
): Answer<"/api/chat">["messages"][number] {
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

function search(params: URLSearchParams, now: number): Answer<"/api/search"> {
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
            score: 0,
            matches: [
              {
                message_id: messageId(chat, shown.position),
                node_id: chat.machine,
                role: shown.entry.role,
                created_at: createdAt(chat, now) + shown.position * 2 * MINUTE,
                copies: [{ message_id: messageId(chat, shown.position), node_id: chat.machine }],
                fusion_score: 0,
                rerank_score: null,
                projection_span: { start_char: 0, end_char: shown.entry.content.length },
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
  return {
    query: params.get("q") ?? "",
    intent: "related-discussion",
    realized_mode: "lexical",
    unit: "chats",
    shared: true,
    results,
    next_offset: null,
    notice:
      "The demo matches the words in your question. Mash itself also matches by meaning, so it finds chats that say the same thing differently.",
  };
}

function chatOf(id: string | null): DemoChat | undefined {
  return chats.find((chat) => chat.id === id);
}
function transcript(params: URLSearchParams, now: number): Answer<"/api/chat"> | Refusal {
  const chat = chatOf(params.get("id"));
  if (!chat) return refuse(404, "That chat is not in the demo's history.");
  const offset = Number(params.get("offset") ?? 0);
  const limit = Number(params.get("limit") ?? 100);
  return { id: chat.id, messages: messages(chat, now).slice(offset, offset + limit) };
}
function view(params: URLSearchParams, now: number): Answer<"/api/view"> | Refusal {
  const id = params.get("id") ?? "";
  const chat = chats.find((candidate) => id.startsWith(`${candidate.id}-`));
  if (!chat) return refuse(404, "That message is not in the demo's history.");
  return { id, messages: messages(chat, now) };
}

function record(entry: (typeof records)[number], now: number): Answer<"/api/record"> {
  const chat = chatOf(entry.source.chat);
  const details =
    entry.kind === "decision"
      ? {
          kind: entry.kind,
          state: "accepted" as const,
          alternatives: [...(entry.alternatives ?? [])],
          consequences: [...(entry.consequences ?? [])],
        }
      : entry.kind === "preference"
        ? { kind: entry.kind, state: "active" as const, conditions: [] }
        : { kind: entry.kind, state: "current" as const, reported_by: "Sam", valid_until: null };
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
function recordList(params: URLSearchParams, now: number): Answer<"/api/records"> {
  const wanted = terms(params.get("q") ?? "");
  const kind = params.get("kind");
  const review = params.get("review");
  const chat = params.get("chat");
  const state = params.get("state");
  const current = ["accepted", "active", "current"];
  return {
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
  };
}

const roots = {
  "claude-code": ["/home/sam/.claude/projects"],
  codex: ["/home/sam/.codex/sessions"],
  cursor: ["/home/sam/.config/Cursor/User/workspaceStorage"],
  copilot: ["/home/sam/.copilot/session-state"],
  opencode: ["/home/sam/.local/share/opencode"],
};
type Rooted = keyof typeof roots;
function perAgent<T>(entry: (paths: string[]) => T): Record<Rooted, T> {
  return {
    "claude-code": entry(roots["claude-code"]),
    codex: entry(roots.codex),
    cursor: entry(roots.cursor),
    copilot: entry(roots.copilot),
    opencode: entry(roots.opencode),
  };
}
function status(): Answer<"/api/status"> {
  const local = chats.filter((chat) => chat.machine === localMachine);
  const count = local.reduce((sum, chat) => sum + chat.messages.length, 0);
  return {
    ready: true,
    chats: local.length,
    messages: count,
    searchable_messages: count,
    models_installed: true,
    realized_mode: "hybrid",
    database_path: "/home/sam/.local/share/mash/mash.sqlite",
    recommended_action: null,
    embeddings: count,
    stored_embeddings: count,
    query_models: "available",
    semantic_support: true,
    installation: { attention: 0, unverified: 0, unconfigured: 0, clients: [] },
    local_artifacts: { metadata: "ready", lexical: "ready", dense: "ready" },
    lexical: null,
    views: null,
    configuration: {
      path: "/home/sam/.config/mash/config.toml",
      loaded: true,
      providers: perAgent((paths) => ({ roots: paths })),
      attachments: { share_files: false, allowed_roots: [] },
      index: { since_days: 90, destination: "both" },
      embeddings: { backend: "local" },
      qdrant: {
        url: "http://127.0.0.1:6333",
        grpc_url: "http://127.0.0.1:6334",
        api_key_env: null,
        memory: "durable",
        vector_search: { algorithm: "hnsw", ef_runtime: 200 },
      },
      views: {
        build: { embedding_text: "dialogue" },
        query: {
          freshness_warnings: true,
          intent: "related-discussion",
          unit: "chats",
          limit: 20,
          fusion: { rank_constant: 60, lexical_weight: 1, semantic_weight: 1, recency_weight: 50 },
          ranking: "contextual-rerank",
        },
      },
      records: { enabled: true, backfill_since: null },
      redaction: { output: true, publication: true },
      shared: {
        group_id: "harbor",
        node_id: localMachine,
        api_url: "https://mash.harbor.example",
        token_env: "MASH_NODE_TOKEN",
      },
    },
  };
}
function settings(): Answer<"/api/services/settings"> {
  return {
    path: "/home/sam/.config/mash/config.toml",
    revision: "demo",
    editable: false,
    agents: perAgent((paths) => ({ indexed: true, roots: paths })),
    since_days: 90,
    destination: "both",
    shared: true,
    memory_capture: true,
    hide_credentials: { output: true, publication: true },
  };
}
/** What a dashboard with no service management reports: this machine's index, nothing to control. */
function machine(now: number): Answer<"/api/services/local"> {
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

type Read<P extends ReadPath> = (params: URLSearchParams, now: number) => Answer<P> | Refusal;

// One entry for every route the contract lists, each answering with that
// route's type: a route added to the server does not compile here until the
// demo answers it or declines to.
const reads: { readonly [P in ReadPath]: Read<P> } = {
  "/api/health": () => ({ build: tools.build_revision }),
  "/api/chats": (_, now) => ({
    local_node: localMachine,
    truncated: false,
    shared_error: null,
    chats: chats.map((chat) => row(chat, now)),
  }),
  "/api/search": search,
  "/api/chat": transcript,
  "/api/view": view,
  "/api/records": recordList,
  "/api/record": (params, now) => {
    const entry = records.find((candidate) => candidate.id === params.get("id"));
    return entry ? record(entry, now) : refuse(404, "That memory is not in the demo.");
  },
  "/api/status": status,
  "/api/console/tools": () => tools,
  "/api/console/call": () => notInDemo("Running tools"),
  "/api/services/access": () => ({ authentication_required: false, local_node: localMachine }),
  "/api/services": (_, now) => ({ machines: [machine(now)] }),
  "/api/services/local": (_, now) => machine(now),
  "/api/services/settings": settings,
  "/api/services/fleet": () => ({ enabled: false, revision: null, actions: [], services: [] }),
  "/api/services/fleet/jobs": () =>
    refuse(400, "Fleet jobs are only available on the configured controller"),
  "/api/services/operations": () => [],
  "/api/services/log": () => refuse(404, "No configured log source for this service"),
  "/api/services/installation": () => ({
    build: tools.build_revision,
    agents: (["claude-code", "codex"] as const).map((name) => ({
      agent: name,
      expected: true,
      connected: true,
      steps: (["plugin", "skill", "mcp"] as const).map((component) => ({
        component,
        state: "present" as const,
        message: "Installed and readable.",
        remediation: null,
      })),
    })),
  }),
};
function isRead(path: string): path is ReadPath {
  return Object.hasOwn(reads, path);
}

/** The answer `mash dash` would give to one request, from the demo's history. */
export function respond(url: string, method: string, now: number): Reply {
  const { pathname, searchParams } = new URL(url, "http://demo.invalid");
  const answer =
    method !== "GET"
      ? notInDemo("Changing anything")
      : isRead(pathname)
        ? reads[pathname](searchParams, now)
        : refuse(404, "This part of Mash is not in the demo.");
  return answer instanceof Refusal
    ? { status: answer.status, body: { error: { message: answer.message } } }
    : { status: 200, body: answer };
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
