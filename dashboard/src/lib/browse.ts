import type { Agent, ChatSummary } from "./types";

/** The folder name people recognise a project by; the full path stays the key. */
export function projectLabel(project: string): string {
  return project.split(/[\\/]/).filter(Boolean).at(-1) ?? "no project";
}

/** A short time for a list row: the clock today, the date otherwise. */
export function whenLabel(ms: number | undefined, now: number): string {
  if (ms === undefined) return "time unknown";
  const date = new Date(ms);
  const today = new Date(now);
  if (date.toDateString() === today.toDateString())
    return date.toLocaleTimeString(undefined, {
      hour: "2-digit",
      minute: "2-digit",
      hour12: false,
    });
  return date.toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
    ...(date.getFullYear() === today.getFullYear() ? {} : { year: "numeric" }),
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
