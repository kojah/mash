import { queryOptions } from "@tanstack/svelte-query";
import { isAgent, read } from "./api";
import { stableRead } from "./queries";
import type { Answer } from "./routes";
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

export function parseStatus(body: Answer<"/api/status">): Status {
  const config = body.configuration;
  return {
    ready: body.ready,
    chats: body.chats,
    messages: body.messages,
    searchableMessages: body.searchable_messages,
    modelsInstalled: body.models_installed,
    mode: body.realized_mode,
    databasePath: body.database_path ?? null,
    nextStep: body.recommended_action ?? null,
    configuration: {
      path: config.path,
      loaded: config.loaded,
      providers: Object.entries(config.providers).flatMap(([agent, entry]) =>
        isAgent(agent) && entry ? [{ agent, roots: entry.roots }] : [],
      ),
      sinceDays: config.index.since_days ?? null,
      destination: config.index.destination,
      embeddings: config.embeddings.backend,
      vectorSearch: config.qdrant.vector_search.algorithm,
      ranking: config.views.query.ranking,
      searchStore: config.qdrant.url,
      recordsEnabled: config.records.enabled,
      redactOutput: config.redaction.output,
      redactPublication: config.redaction.publication,
      shared: config.shared
        ? {
            group: config.shared.group_id,
            node: config.shared.node_id,
            url: config.shared.api_url,
          }
        : null,
    },
  };
}

export const statusOptions = queryOptions({
  ...stableRead,
  queryKey: ["status"],
  queryFn: async ({ signal }) =>
    parseStatus(await read("/api/status", new URLSearchParams(), signal)),
});
