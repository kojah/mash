/**
 * The one place a request leaves the dashboard. `mash dash` answers by
 * default; the demo build installs a transport that answers in the browser,
 * so nothing else in the dashboard knows which of the two it is talking to.
 */
export type Transport = (path: string, init: RequestInit) => Promise<Response>;

let transport: Transport = (path, init) => fetch(path, init);

export function install(next: Transport): void {
  transport = next;
}
export function send(path: string, init: RequestInit): Promise<Response> {
  return transport(path, init);
}

/** The part of `EventSource` the dashboard uses to hear that the index changed. */
export interface EventStream {
  onopen: ((event: Event) => void) | null;
  onerror: ((event: Event) => void) | null;
  addEventListener(type: string, listener: (event: MessageEvent<string>) => void): void;
  close(): void;
}
export type Events = (path: string) => EventStream;

let events: Events | undefined =
  typeof EventSource === "undefined" ? undefined : (path) => new EventSource(path);

/** Replaces how the event stream is opened; `undefined` means there is none, as in the demo. */
export function installEvents(next: Events | undefined): void {
  events = next;
}
export function listen(path: string): EventStream | undefined {
  return events?.(path);
}
