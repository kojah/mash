// Hash routes keep the static bundle servable from any path: #/ and #/chat/<id>.
export type Route =
  | { readonly page: "home" }
  | { readonly page: "records"; readonly params: string }
  | { readonly page: "record"; readonly id: string; readonly node: string; readonly back: string }
  | {
      readonly page: "chat";
      readonly id: string;
      readonly node: string;
      readonly message: string;
      readonly shared: boolean;
    };

export function parseRoute(hash: string): Route {
  if (/^#\/records(?:\?|$)/.test(hash))
    return { page: "records", params: new URLSearchParams(hash.split("?")[1]).toString() };
  const record = /^#\/record\/([^/?#]+)(?:\?|$)/.exec(hash);
  const match = /^#\/chat\/([^/?#]+)/.exec(hash);
  try {
    const params = new URLSearchParams(hash.split("?")[1]);
    if (record?.[1]) {
      const back = params.get("back") ?? "";
      return {
        page: "record",
        id: decodeURIComponent(record[1]),
        node: params.get("node") ?? "",
        back: /^#\/records(?:\?|$)/.test(back) ? back : "#/records",
      };
    }
    return match?.[1]
      ? {
          page: "chat",
          id: decodeURIComponent(match[1]),
          node: params.get("node") ?? "",
          message: params.get("message") ?? "",
          shared: params.get("shared") === "true",
        }
      : { page: "home" };
  } catch {
    return { page: "home" };
  }
}

export function chatHref(id: string, node = "", message = "", shared = false): string {
  return `#/chat/${encodeURIComponent(id)}?${new URLSearchParams({ node, message, ...(shared ? { shared: "true" } : {}) })}`;
}

export function recordsHref(params = new URLSearchParams()): string {
  return `#/records?${params}`;
}
export function recordHref(id: string, node = "", back = "#/records"): string {
  return `#/record/${encodeURIComponent(id)}?${new URLSearchParams({ node, back })}`;
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
