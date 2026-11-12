import { isAgent } from "./api";
import type { Agent, SearchFilters } from "./types";

export const dateRanges = [
  { days: null, label: "any time" },
  { days: 1, label: "today" },
  { days: 7, label: "past week" },
  { days: 31, label: "past month" },
] as const;
export const sorts = [
  { value: "new", label: "newest" },
  { value: "old", label: "oldest" },
] as const;
export type Sort = (typeof sorts)[number]["value"];

/**
 * A search as the address bar states it. Results are read from this, so the
 * controls on the page stay drafts until they are applied.
 */
export interface Applied {
  readonly query: string;
  readonly agents: readonly Agent[];
  readonly machines: readonly string[];
  readonly projects: readonly string[];
  readonly days: number | null;
  /** Order for a chat list; a query is always ranked by relevance. */
  readonly sort: Sort;
  /** Zero-based position of the first result shown. */
  readonly offset: number;
}

function list(params: URLSearchParams, name: string): string[] {
  return (params.get(name) ?? "").split(",").filter(Boolean);
}

export function parseApplied(params: string): Applied {
  const values = new URLSearchParams(params);
  const days = Number(values.get("days"));
  const offset = Number(values.get("offset"));
  return {
    query: (values.get("q") ?? "").trim(),
    agents: list(values, "agent").filter(isAgent),
    machines: list(values, "machine"),
    projects: list(values, "project"),
    days: dateRanges.some((range) => range.days === days) && days > 0 ? days : null,
    sort: values.get("sort") === "old" ? "old" : "new",
    offset: Number.isSafeInteger(offset) && offset > 0 ? offset : 0,
  };
}

/** `base` is the page the search belongs to: the chat list, or the export page. */
export function appliedHref(applied: Applied, base = "#/search"): string {
  const params = new URLSearchParams();
  if (applied.query) params.set("q", applied.query);
  if (applied.agents.length) params.set("agent", applied.agents.join(","));
  if (applied.machines.length) params.set("machine", applied.machines.join(","));
  if (applied.projects.length) params.set("project", applied.projects.join(","));
  if (applied.days !== null) params.set("days", String(applied.days));
  if (applied.sort !== "new") params.set("sort", applied.sort);
  if (applied.offset) params.set("offset", String(applied.offset));
  return params.size ? `${base}?${params}` : base;
}

export function filtersOf(applied: Applied): SearchFilters {
  return {
    agents: applied.agents,
    machines: applied.machines,
    projects: applied.projects,
    days: applied.days,
  };
}

/** Whether two searches would return the same results, ignoring the page. */
export function sameSearch(a: Applied, b: Applied): boolean {
  return appliedHref({ ...a, offset: 0 }) === appliedHref({ ...b, offset: 0 });
}
