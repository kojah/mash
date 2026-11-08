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

function object(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw new Error("Mash returned an invalid response. Reload after updating the dashboard.");
  return value as Record<string, unknown>;
}
function string(value: unknown): string {
  if (typeof value !== "string") throw new Error("Mash returned an invalid text field.");
  return value;
}
function optionalString(value: unknown): string | undefined {
  return value == null ? undefined : string(value);
}
function number(value: unknown): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value))
    throw new Error("Mash returned an invalid number.");
  return value;
}
function array(value: unknown): unknown[] {
  if (!Array.isArray(value)) throw new Error("Mash returned an invalid list.");
  return value;
}
export function isAgent(value: string): value is Agent {
  return ["claude-code", "codex", "opencode", "copilot", "cursor", "antigravity"].includes(value);
}
function timestamp(value: unknown): number | undefined {
  return value == null ? undefined : number(value);
}
function date(value: number | undefined): string {
  return value === undefined ? "Time unknown" : new Date(value).toLocaleString();
}
function chat(value: unknown, localNode: string): ChatSummary {
  const row = object(value);
  const agent = string(row.provider);
  if (!isAgent(agent)) throw new Error(`Unsupported agent: ${agent}`);
  const updatedMs = timestamp(row.updated_at) ?? timestamp(row.created_at);
  return {
    id: string(row.chat_id),
    title: optionalString(row.title) || "Untitled chat",
    agent,
    machine: optionalString(row.node_id) ?? localNode,
    project: optionalString(row.project) ?? "",
    updatedAt: date(updatedMs),
    ...(updatedMs === undefined ? {} : { updatedMs }),
  };
}
export async function read(
  path: string,
  params: URLSearchParams,
  signal: AbortSignal,
): Promise<unknown> {
  const response = await fetch(`${path}?${params}`, {
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
      const error: unknown = await response.json();
      detail = string(object(object(error).error).message);
    } catch {
      /* Non-JSON HTTP failures keep the safe fallback. */
    }
    throw new Error(`Mash request failed (${response.status}). ${detail}`);
  }
  const body: unknown = await response.json();
  return body;
}
export function parseCatalog(value: unknown): Catalog {
  const body = object(value);
  const localNode = string(body.local_node);
  if (typeof body.truncated !== "boolean") throw new Error("Invalid catalogue completeness.");
  const sharedError = optionalString(body.shared_error);
  return {
    localNode,
    chats: array(body.chats).map((row) => chat(row, localNode)),
    warnings: [
      ...(sharedError ? [`Shared history is unavailable: ${sharedError}`] : []),
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

export function parseSearch(value: unknown, localNode: string): SearchPage {
  const body = object(value);
  return {
    hits: array(body.results).map((value) => {
      const row = object(value);
      const first = array(row.matches)[0];
      const match = first === undefined ? undefined : object(first);
      const lookup = match?.lookup == null ? undefined : object(object(match.lookup).view);
      const metadata = chat(row, localNode);
      const machine = lookup
        ? (optionalString(lookup.shared) ?? localNode)
        : (optionalString(match?.node_id) ?? metadata.machine);
      return {
        chat: { ...metadata, machine },
        messageId: lookup ? string(lookup.id) : match ? string(match.message_id) : "",
        before: match ? string(match.snippet) : "",
        match: "",
        after: "",
      };
    }),
    nextOffset: body.next_offset == null ? null : number(body.next_offset),
    warnings: [body.shared_error, body.shared_context_error, body.notice].flatMap((value) => {
      const text = optionalString(value);
      return text ? [text] : [];
    }),
  };
}
export function parseMessages(value: unknown): MessagePage {
  return {
    messages: array(object(value).messages).map((value) => {
      const row = object(value);
      const tools = row.tools == null ? {} : object(row.tools);
      const calls = tools.tool_calls == null ? [] : array(tools.tool_calls).map(object);
      const results = tools.tool_results == null ? [] : array(tools.tool_results).map(object);
      const content = string(row.content);
      const role = string(row.role);
      const presentation =
        results.length || role === "tool"
          ? "tool-result"
          : calls.length
            ? "tool-call"
            : content.startsWith("[Thinking]")
              ? "reasoning"
              : "dialogue";
      return {
        id: string(row.id),
        ordinal: number(row.ordinal),
        role,
        presentation,
        toolLabel:
          calls.map((call) => optionalString(call.name) ?? "Tool").join(", ") || "Tool output",
        at: date(timestamp(row.created_at)),
        blocks: [{ type: "text", markdown: content }],
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
