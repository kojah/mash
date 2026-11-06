// Hash routes keep the static bundle servable from any path: #/ and #/chat/<id>.
export type Route =
  | { readonly page: "home" }
  | { readonly page: "chat"; readonly id: string; readonly node: string; readonly message: string };

export function parseRoute(hash: string): Route {
  const match = /^#\/chat\/([^/?#]+)/.exec(hash);
  try {
    const params = new URLSearchParams(hash.split("?")[1]);
    return match?.[1]
      ? {
          page: "chat",
          id: decodeURIComponent(match[1]),
          node: params.get("node") ?? "",
          message: params.get("message") ?? "",
        }
      : { page: "home" };
  } catch {
    return { page: "home" };
  }
}

export function chatHref(id: string, node = "", message = ""): string {
  return `#/chat/${encodeURIComponent(id)}?${new URLSearchParams({ node, message })}`;
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
