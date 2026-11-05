// Hash routes keep the static bundle servable from any path: #/ and #/chat/<id>.
export type Route = { readonly page: "home" } | { readonly page: "chat"; readonly id: string };

export function parseRoute(hash: string): Route {
  const match = /^#\/chat\/([^/?#]+)/.exec(hash);
  return match?.[1] ? { page: "chat", id: decodeURIComponent(match[1]) } : { page: "home" };
}

export function chatHref(id: string): string {
  return `#/chat/${encodeURIComponent(id)}`;
}

class Router {
  current = $state<Route>(parseRoute(globalThis.location?.hash ?? ""));

  constructor() {
    globalThis.addEventListener?.("hashchange", () => {
      this.current = parseRoute(location.hash);
    });
  }
}

export const router = new Router();
