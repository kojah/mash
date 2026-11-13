// Hears from `mash dash` that history changed, on this machine or on one
// that publishes to the shared index, over one Server-Sent Events stream per
// tab. An event names chats, never their text: whatever is on screen rereads
// itself through the ordinary history routes.
import { listen } from "./transport";

export interface Change {
  /** The machine whose published history changed; null for this machine's own index. */
  readonly node: string | null;
  /** Chats that were added, removed or changed. Empty when `all` is set. */
  readonly chats: readonly string[];
  /** Too much changed to name, or changes may have been missed: reread everything shown. */
  readonly all: boolean;
  /** Whether that machine's memories may have changed. */
  readonly records: boolean;
}
const EVERYTHING: Change = { node: null, chats: [], all: true, records: true };

/** One event's data, or undefined when it is not what `/api/events` sends. */
export function parseChange(data: string): Change | undefined {
  let body: unknown;
  try {
    body = JSON.parse(data);
  } catch {
    return undefined;
  }
  if (typeof body !== "object" || body === null) return undefined;
  const { node, chats, all, records } = body as Record<string, unknown>;
  if (typeof all !== "boolean" || typeof records !== "boolean" || !Array.isArray(chats))
    return undefined;
  if (node !== null && typeof node !== "string") return undefined;
  const ids = chats.filter((id): id is string => typeof id === "string");
  return ids.length === chats.length ? { node, chats: ids, all, records } : undefined;
}

/**
 * Whether a change is news for the chat `id` shown from `machine`. `local`
 * says the chat is read from this machine's own index rather than the shared one.
 */
export function concerns(change: Change, id: string, machine: string, local: boolean): boolean {
  if (change.all) return true;
  if (!change.chats.includes(id)) return false;
  // A dashboard without an index of its own hears of its own node by name.
  return change.node === null ? local : change.node === machine;
}

const listeners = new Set<(change: Change) => void>();

/** Calls `listener` for every change until the returned function is called. */
export function onChange(listener: (change: Change) => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}
function publish(change: Change): void {
  for (const listener of listeners) listener(change);
}

/** How long to wait before the next attempt: 1s, doubling, at most 30s. */
export function retryDelay(failures: number): number {
  return Math.min(30_000, 1000 * 2 ** Math.max(0, failures - 1));
}

/**
 * Keeps the stream open until the returned function is called. A stream that
 * drops is reopened after a growing wait, and once it is back everything is
 * reread, since changes in between went unheard. `lost` is called when a
 * working stream first fails.
 */
export function connect(lost: () => void = () => {}): () => void {
  let stream = listen("/api/events");
  let timer: ReturnType<typeof setTimeout> | undefined;
  let failures = 0;
  let heard = false;
  let stopped = false;
  const watch = (current: NonNullable<typeof stream>): void => {
    current.onopen = () => {
      failures = 0;
      if (heard) publish(EVERYTHING);
      heard = true;
    };
    current.addEventListener("change", (event) => {
      const change = parseChange(event.data);
      if (change) publish(change);
    });
    current.onerror = () => {
      // The browser's own retry neither backs off nor survives an HTTP error.
      current.close();
      if (stopped) return;
      failures += 1;
      if (failures === 1) lost();
      timer = setTimeout(() => {
        stream = listen("/api/events");
        if (stream) watch(stream);
      }, retryDelay(failures));
    };
  };
  if (stream) watch(stream);
  return () => {
    stopped = true;
    clearTimeout(timer);
    stream?.close();
  };
}

/** Runs `run` at once, then at most once per `wait` while calls keep coming. */
export function throttle(run: () => void, wait: number): { (): void; cancel(): void } {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let again = false;
  const call = (): void => {
    if (timer !== undefined) {
      again = true;
      return;
    }
    run();
    timer = setTimeout(() => {
      timer = undefined;
      if (again) {
        again = false;
        call();
      }
    }, wait);
  };
  call.cancel = () => {
    clearTimeout(timer);
    timer = undefined;
    again = false;
  };
  return call;
}

/** What a reread page is to the one on screen. */
export type PageUpdate =
  | { readonly kind: "same" }
  /** Everything shown is unchanged; `added` messages follow, `onPage` if this page gained any. */
  | { readonly kind: "appended"; readonly added: number; readonly onPage: boolean }
  /** Something already shown differs. */
  | { readonly kind: "rewritten" };

interface Page {
  readonly messages: readonly unknown[];
  readonly total?: number;
}

/** Compares the page on screen with the same page read again. */
export function pageUpdate(shown: Page, next: Page): PageUpdate {
  if (next.messages.length < shown.messages.length) return { kind: "rewritten" };
  for (const [index, message] of shown.messages.entries())
    if (JSON.stringify(message) !== JSON.stringify(next.messages[index]))
      return { kind: "rewritten" };
  const onPage = next.messages.length - shown.messages.length;
  const added =
    shown.total !== undefined && next.total !== undefined ? next.total - shown.total : onPage;
  if (added < 0) return { kind: "rewritten" };
  if (added === 0 && onPage === 0) return { kind: "same" };
  return { kind: "appended", added: Math.max(added, onPage), onPage: onPage > 0 };
}
