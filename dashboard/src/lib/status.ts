import { queryOptions } from "@tanstack/svelte-query";
import { isAgent, read } from "./api";
import { stableRead } from "./queries";
import type { Agent } from "./types";

/** What `mash status` reports about this machine, for the settings page. */
export interface Status {
  readonly ready: boolean;
  readonly chats: number;
  readonly messages: number;
  readonly searchableMessages: number;
  readonly modelsInstalled: boolean;
  readonly mode: string;
  readonly databasePath: string | null;
  /** The command that would make progress, when something needs doing. */
  readonly nextStep: string | null;
  readonly configuration: {
    readonly path: string;
    /** False when no file exists and built-in defaults apply. */
    readonly loaded: boolean;
    readonly providers: readonly { readonly agent: Agent; readonly roots: readonly string[] }[];
    readonly sinceDays: number | null;
    readonly destination: string;
    readonly embeddings: string;
    readonly vectorSearch: string;
    readonly ranking: string;
    readonly searchStore: string;
    readonly recordsEnabled: boolean;
    readonly redactOutput: boolean;
    readonly redactPublication: boolean;
    readonly shared: { readonly group: string; readonly node: string; readonly url: string } | null;
  };
}

function fail(field: string): never {
  throw new Error(`Mash returned an invalid status (${field}). Reload after updating.`);
}
function object(value: unknown, field: string): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) fail(field);
  return value as Record<string, unknown>;
}
function text(value: unknown, field: string): string {
  return typeof value === "string" ? value : fail(field);
}
function flag(value: unknown, field: string): boolean {
  return typeof value === "boolean" ? value : fail(field);
}
function count(value: unknown, field: string): number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0
    ? value
    : fail(field);
}

export function parseStatus(value: unknown): Status {
  const body = object(value, "status");
  const config = object(body.configuration, "configuration");
  const qdrant = object(config.qdrant, "qdrant");
  const vector = object(qdrant.vector_search, "vector_search");
  const index = object(config.index, "index");
  const redaction = object(config.redaction, "redaction");
  const shared = config.shared == null ? null : object(config.shared, "shared");
  const views = object(config.views, "views");
  const action = body.recommended_action;
  return {
    ready: flag(body.ready, "ready"),
    chats: count(body.chats, "chats"),
    messages: count(body.messages, "messages"),
    searchableMessages: count(body.searchable_messages, "searchable_messages"),
    modelsInstalled: flag(body.models_installed, "models_installed"),
    mode: text(body.realized_mode, "realized_mode"),
    databasePath: body.database_path == null ? null : text(body.database_path, "database_path"),
    nextStep: typeof action === "string" ? action : null,
    configuration: {
      path: text(config.path, "path"),
      loaded: flag(config.loaded, "loaded"),
      providers: Object.entries(object(config.providers, "providers")).flatMap(([agent, entry]) => {
        const roots = object(entry, "provider").roots;
        return isAgent(agent) && Array.isArray(roots)
          ? [{ agent, roots: roots.map((root) => text(root, "root")) }]
          : [];
      }),
      sinceDays: index.since_days == null ? null : count(index.since_days, "since_days"),
      destination: text(index.destination, "destination"),
      embeddings: text(object(config.embeddings, "embeddings").backend, "backend"),
      vectorSearch: text(vector.algorithm, "algorithm"),
      ranking: text(object(views.query, "query").ranking, "ranking"),
      searchStore: text(qdrant.url, "qdrant url"),
      recordsEnabled: flag(object(config.records, "records").enabled, "records"),
      redactOutput: flag(redaction.output, "redaction"),
      redactPublication: flag(redaction.publication, "redaction"),
      shared: shared && {
        group: text(shared.group_id, "group_id"),
        node: text(shared.node_id, "node_id"),
        url: text(shared.api_url, "api_url"),
      },
    },
  };
}

export const statusOptions = queryOptions({
  ...stableRead,
  queryKey: ["status"],
  queryFn: async ({ signal }) =>
    parseStatus(await read("/api/status", new URLSearchParams(), signal)),
});
