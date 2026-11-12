// The settings a person changes from the dashboard, as the server offers
// them. History locations are found by the machine itself; the page only says
// which agents to index.
import { queryOptions } from "@tanstack/svelte-query";
import { isAgent } from "./api";
import { stableRead } from "./queries";
import { request } from "./services.svelte";
import type { Agent } from "./types";

export type Destination = "local" | "both";
export interface Settings {
  readonly path: string;
  /** Names the file's contents; a change is made against one revision. */
  readonly revision: string;
  /** False when this dashboard shows settings but does not accept changes. */
  readonly editable: boolean;
  readonly agents: readonly {
    readonly agent: Agent;
    readonly indexed: boolean;
    readonly roots: readonly string[];
  }[];
  readonly sinceDays: number | null;
  readonly destination: Destination;
  /** Whether a shared server is configured for history to go to. */
  readonly shared: boolean;
  readonly memoryCapture: boolean;
  readonly hideInResults: boolean;
  readonly hideWhenPublishing: boolean;
}
/** What the page has chosen, which may differ from the file. */
export interface Draft {
  agents: Partial<Record<Agent, boolean>>;
  sinceDays: number | null;
  destination: Destination;
  memoryCapture: boolean;
  hideInResults: boolean;
  hideWhenPublishing: boolean;
}

function fail(field: string): never {
  throw new Error(`Mash returned invalid settings (${field}). Reload after updating.`);
}
function fields(value: unknown, field: string): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) fail(field);
  return value as Record<string, unknown>;
}
function flag(value: unknown, field: string): boolean {
  return typeof value === "boolean" ? value : fail(field);
}
function text(value: unknown, field: string): string {
  return typeof value === "string" ? value : fail(field);
}

export function parseSettings(value: unknown): Settings {
  const body = fields(value, "settings");
  const hide = fields(body.hide_credentials, "hide_credentials");
  const days = body.since_days;
  if (days !== null && !(typeof days === "number" && Number.isSafeInteger(days) && days > 0))
    fail("since_days");
  const destination = body.destination;
  if (destination !== "local" && destination !== "both") fail("destination");
  return {
    path: text(body.path, "path"),
    revision: text(body.revision, "revision"),
    editable: flag(body.editable, "editable"),
    agents: Object.entries(fields(body.agents, "agents")).flatMap(([agent, entry]) => {
      if (!isAgent(agent)) return [];
      const row = fields(entry, "agent");
      const roots = Array.isArray(row.roots) ? row.roots : fail("roots");
      return [
        { agent, indexed: flag(row.indexed, "indexed"), roots: roots.map((r) => text(r, "root")) },
      ];
    }),
    sinceDays: days,
    destination,
    shared: flag(body.shared, "shared"),
    memoryCapture: flag(body.memory_capture, "memory_capture"),
    hideInResults: flag(hide.output, "output"),
    hideWhenPublishing: flag(hide.publication, "publication"),
  };
}

export const settingsOptions = queryOptions({
  ...stableRead,
  queryKey: ["settings"],
  queryFn: async ({ signal }) => parseSettings(await request("/api/services/settings", signal)),
});

export function draftOf(settings: Settings): Draft {
  return {
    agents: Object.fromEntries(settings.agents.map((entry) => [entry.agent, entry.indexed])),
    sinceDays: settings.sinceDays,
    destination: settings.destination,
    memoryCapture: settings.memoryCapture,
    hideInResults: settings.hideInResults,
    hideWhenPublishing: settings.hideWhenPublishing,
  };
}

export interface Difference {
  /** The setting's row on the page. */
  readonly key: string;
  readonly label: string;
  readonly from: string;
  readonly to: string;
}
const onOff = (value: boolean) => (value ? "on" : "off");
const range = (days: number | null) => (days === null ? "all history" : `the last ${days} days`);
export const destinations: Record<Destination, string> = {
  local: "this machine only",
  both: "this machine and the shared server",
};

/** What saving the draft would change, in the words the page uses. */
export function differences(
  settings: Settings,
  draft: Draft,
  names: Record<Agent, string>,
): Difference[] {
  const changed: Difference[] = [];
  for (const entry of settings.agents) {
    const wanted = draft.agents[entry.agent] ?? entry.indexed;
    if (wanted !== entry.indexed)
      changed.push({
        key: `agent:${entry.agent}`,
        label: `index ${names[entry.agent]}`,
        from: onOff(entry.indexed),
        to: onOff(wanted),
      });
  }
  const row = (key: string, label: string, from: string, to: string) => {
    if (from !== to) changed.push({ key, label, from, to });
  };
  row("range", "history range", range(settings.sinceDays), range(draft.sinceDays));
  row(
    "destination",
    "index into",
    destinations[settings.destination],
    destinations[draft.destination],
  );
  row("memory", "memory capture", onOff(settings.memoryCapture), onOff(draft.memoryCapture));
  row(
    "results",
    "hide credentials in results",
    onOff(settings.hideInResults),
    onOff(draft.hideInResults),
  );
  row(
    "publishing",
    "hide credentials when publishing",
    onOff(settings.hideWhenPublishing),
    onOff(draft.hideWhenPublishing),
  );
  return changed;
}

/** Why the draft cannot be saved as it stands, or an empty string. */
export function problem(settings: Settings, draft: Draft): string {
  if (!settings.agents.some((entry) => draft.agents[entry.agent] ?? entry.indexed))
    return "Keep at least one agent's history indexed.";
  if (
    draft.sinceDays !== null &&
    !(Number.isSafeInteger(draft.sinceDays) && draft.sinceDays >= 1 && draft.sinceDays <= 36_500)
  )
    return "History range must be between 1 and 36,500 days.";
  return "";
}

export async function saveSettings(settings: Settings, draft: Draft): Promise<Settings> {
  return parseSettings(
    await request("/api/services/settings", undefined, {
      revision: settings.revision,
      agents: draft.agents,
      since_days: draft.sinceDays,
      destination: draft.destination,
      memory_capture: draft.memoryCapture,
      hide_credentials: { output: draft.hideInResults, publication: draft.hideWhenPublishing },
    }),
  );
}
