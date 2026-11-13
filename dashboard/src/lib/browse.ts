import type { Agent, ChatSummary } from "./types";

/** The folder name people recognise a project by; the full path stays the key. */
export function projectLabel(project: string): string {
  return project.split(/[\\/]/).filter(Boolean).at(-1) ?? "no project";
}

/**
 * How long ago, for a list row: `3m`, `3h` or `3d`, and the date once it is
 * more than thirty days back.
 */
export function whenLabel(ms: number | undefined, now: number): string {
  if (ms === undefined) return "time unknown";
  const minutes = Math.floor((now - ms) / 60_000);
  // A clock a little ahead of this one still reads as the present.
  if (minutes < 1) return "now";
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h`;
  const days = Math.floor(hours / 24);
  if (days <= 30) return `${days}d`;
  const date = new Date(ms);
  return date.toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
    ...(date.getFullYear() === new Date(now).getFullYear() ? {} : { year: "numeric" }),
  });
}

/** Chats that share one value of a field, with what else is true of them. */
export interface Group {
  readonly key: string;
  readonly chats: readonly ChatSummary[];
  readonly latestMs: number | undefined;
  readonly agents: readonly (readonly [Agent, number])[];
  readonly machines: readonly (readonly [string, number])[];
  readonly projects: readonly (readonly [string, number])[];
}

function tally<T>(values: readonly T[]): (readonly [T, number])[] {
  const counts = new Map<T, number>();
  for (const value of values) counts.set(value, (counts.get(value) ?? 0) + 1);
  return [...counts].sort((a, b) => b[1] - a[1]);
}

/** Groups in descending size; chats keep the catalogue's newest-first order. */
export function groupChats(
  chats: readonly ChatSummary[],
  key: (chat: ChatSummary) => string,
): Group[] {
  const groups = new Map<string, ChatSummary[]>();
  for (const chat of chats) {
    const name = key(chat);
    const group = groups.get(name);
    if (group) group.push(chat);
    else groups.set(name, [chat]);
  }
  return [...groups]
    .map(([name, members]) => ({
      key: name,
      chats: members,
      latestMs: members.reduce<number | undefined>(
        (latest, chat) =>
          chat.updatedMs !== undefined && (latest === undefined || chat.updatedMs > latest)
            ? chat.updatedMs
            : latest,
        undefined,
      ),
      agents: tally(members.map((chat) => chat.agent)),
      machines: tally(members.map((chat) => chat.machine)),
      projects: tally(members.map((chat) => chat.project)),
    }))
    .sort((a, b) => b.chats.length - a.chats.length || a.key.localeCompare(b.key));
}

export const projectSorts = ["chats", "recent", "name"] as const;
export type ProjectSort = (typeof projectSorts)[number];
/** What the Projects page is asked to show: a name to find, an agent and a
 * machine to keep, and an order. An empty string means no restriction. */
export interface ProjectView {
  find: string;
  agent: string;
  machine: string;
  sort: ProjectSort;
}

/** The projects a view asks for, in its order. Projects that tie keep the
 * order they came in, which is largest first. */
export function arrangeProjects(projects: readonly Group[], view: ProjectView): Group[] {
  const find = view.find.trim().toLowerCase();
  const kept = projects.filter(
    (project) =>
      (!find || projectLabel(project.key).toLowerCase().includes(find)) &&
      (!view.agent || project.agents.some(([agent]) => agent === view.agent)) &&
      (!view.machine || project.machines.some(([machine]) => machine === view.machine)),
  );
  if (view.sort === "recent") return kept.sort((a, b) => (b.latestMs ?? 0) - (a.latestMs ?? 0));
  if (view.sort === "name")
    return kept.sort((a, b) => projectLabel(a.key).localeCompare(projectLabel(b.key)));
  return kept;
}
