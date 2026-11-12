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
