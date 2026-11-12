import type { Answer, ReadPath, Wire } from "./routes";
import { send } from "./transport";
import type { Agent, ChatSummary, Message, SearchFilters, SearchHit } from "./types";

export interface Catalog {
  readonly chats: readonly ChatSummary[];
  readonly localNode: string;
  readonly warnings: readonly string[];
}
export interface SearchPage {
  readonly hits: readonly SearchHit[];
  readonly nextOffset: number | null;
  readonly warnings: readonly string[];
}
export interface MessagePage {
  readonly messages: readonly Message[];
}

export function isAgent(value: string): value is Agent {
  return ["claude-code", "codex", "opencode", "copilot", "cursor", "antigravity"].includes(value);
}
function date(value: number | undefined): string {
  return value === undefined ? "Time unknown" : new Date(value).toLocaleString();
}
function chat(row: Wire["Chat"] | Wire["ChatHit"], localNode: string): ChatSummary {
  const updatedMs = row.updated_at ?? row.created_at ?? undefined;
  return {
    id: row.chat_id,
    title: row.title || "Untitled chat",
    agent: row.provider,
    machine: row.node_id ?? localNode,
    project: row.project ?? "",
    updatedAt: date(updatedMs),
    ...(updatedMs === undefined ? {} : { updatedMs }),
  };
}
export async function read<P extends ReadPath>(
  path: P,
  params: URLSearchParams,
  signal: AbortSignal,
): Promise<Answer<P>> {
  const response = await send(`${path}?${params}`, {
    signal: AbortSignal.any([signal, AbortSignal.timeout(20_000)]),
    cache: "no-store",
  }).catch((error: unknown) => {
    if (signal.aborted) throw error;
    if (error instanceof Error && error.name === "TimeoutError")
      throw new Error("Mash took too long to respond. Try again in a moment.", { cause: error });
    throw new Error(
      "Could not reach Mash. Check your connection and that the dashboard server is running, then retry.",
      { cause: error },
    );
  });
  if (!response.ok) {
    let detail = "Check that mash dash and the requested history source are available.";
    try {
      const failure = (await response.json()) as Wire["Failure"];
      detail = failure.error.message;
    } catch {
      /* Non-JSON HTTP failures keep the safe fallback. */
    }
    throw new Error(`Mash request failed (${response.status}). ${detail}`);
  }
  // The server generates the contract these types come from, and the dashboard
  // is built into the same binary, so the two cannot be out of step.
  return (await response.json()) as Answer<P>;
}
export function parseCatalog(body: Answer<"/api/chats">): Catalog {
  return {
    localNode: body.local_node,
    chats: body.chats.map((row) => chat(row, body.local_node)),
    warnings: [
      ...(body.shared_error ? [`Shared history is unavailable: ${body.shared_error}`] : []),
      ...(body.truncated
        ? ["Showing the newest 10,000 chats. Counts and filter choices cover this catalogue only."]
        : []),
    ],
  };
}
export async function catalog(signal: AbortSignal): Promise<Catalog> {
  return parseCatalog(await read("/api/chats", new URLSearchParams(), signal));
}
/** Empty submissions filter one catalogue snapshot; drafts never alter submitted results. */
export function browse(catalog: Catalog, filters: SearchFilters, now: number): SearchPage {
  return {
    hits: catalog.chats
      .filter(
        (chat) =>
          (!filters.agents.length || filters.agents.includes(chat.agent)) &&
          (!filters.machines.length || filters.machines.includes(chat.machine)) &&
          (!filters.projects.length || filters.projects.includes(chat.project)) &&
          (filters.days === null ||
            (chat.updatedMs !== undefined && chat.updatedMs >= now - filters.days * 86_400_000)),
      )
      .map((chat) => ({ chat, messageId: "", before: "", match: "", after: "" })),
    nextOffset: null,
    warnings: catalog.warnings,
  };
}
export async function search(
  query: string,
  filters: SearchFilters,
  offset: number,
  localNode: string,
  signal: AbortSignal,
): Promise<SearchPage> {
  const params = new URLSearchParams({
    q: query,
    provider: filters.agents.join(","),
    machine: filters.machines.join(","),
    project: filters.projects.join(","),
    offset: String(offset),
  });
  if (filters.days !== null) params.set("days", String(filters.days));
  return parseSearch(await read("/api/search", params, signal), localNode);
}

export function parseSearch(body: Answer<"/api/search">, localNode: string): SearchPage {
  return {
    // The dashboard asks for chats; a result without matches is a message hit.
    hits: body.results.flatMap((row) => {
      if (!("matches" in row)) return [];
      const match = row.matches[0];
      const lookup = match?.lookup?.view;
      const metadata = chat(row, localNode);
      const machine = lookup ? (lookup.shared ?? localNode) : (match?.node_id ?? metadata.machine);
      return [
        {
          chat: { ...metadata, machine },
          messageId: lookup ? lookup.id : (match?.message_id ?? ""),
          before: match?.snippet ?? "",
          match: "",
          after: "",
        },
      ];
    }),
    nextOffset: body.next_offset ?? null,
    warnings: [body.shared_error, body.shared_context_error, body.notice].flatMap((text) =>
      text ? [text] : [],
    ),
  };
}
export function parseMessages(body: Answer<"/api/chat">): MessagePage {
  return {
    messages: body.messages.map((row) => {
      const tools = row.tools ?? {};
      const calls = tools.tool_calls ?? [];
      const results = tools.tool_results ?? [];
      const presentation =
        results.length || row.role === "tool"
          ? "tool-result"
          : calls.length
            ? "tool-call"
            : row.content.startsWith("[Thinking]")
              ? "reasoning"
              : "dialogue";
      return {
        id: row.id,
        ordinal: row.ordinal,
        role: row.role,
        presentation,
        toolLabel: calls.map((call) => call.name ?? "Tool").join(", ") || "Tool output",
        at: date(row.created_at ?? undefined),
        blocks: [{ type: "text", markdown: row.content }],
        tools: {
          calls: calls.map((call) => ({
            name: call.name ?? undefined,
            callId: call.call_id ?? undefined,
            input: call.input ?? undefined,
          })),
          results: results.map((result) => ({
            callId: result.call_id ?? undefined,
            isError: result.is_error ?? undefined,
          })),
        },
        // Opaque provider inputs remain text, never interpreted as commands or markup.
        activity: Object.keys(tools).length ? JSON.stringify(tools, null, 2) : "",
      };
    }),
  };
}
export const PAGE_SIZE = 100;
export async function transcript(
  id: string,
  node: string,
  offset: number,
  signal: AbortSignal,
  shared = false,
): Promise<MessagePage> {
  return parseMessages(
    await read(
      "/api/chat",
      new URLSearchParams({
        id,
        ...(node ? { node } : {}),
        ...(shared ? { shared: "true" } : {}),
        offset: String(offset),
        limit: String(PAGE_SIZE),
      }),
      signal,
    ),
  );
}
export async function context(
  id: string,
  node: string,
  signal: AbortSignal,
  shared = false,
): Promise<MessagePage> {
  const params = new URLSearchParams({ id, context: "25" });
  if (node) params.set("node", node);
  if (shared) params.set("shared", "true");
  return parseMessages(await read("/api/view", params, signal));
}
