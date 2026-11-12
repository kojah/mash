// Hash routes keep the static bundle servable from any path: #/ and #/chat/<id>.
export type Route =
  | { readonly page: "overview" }
  /** Chats and search share one page: an empty query lists chats. */
  | { readonly page: "search"; readonly params: string }
  | { readonly page: "history" }
  /** Bulk export of the chats a set of filters matches. */
  | { readonly page: "export"; readonly params: string }
  | { readonly page: "projects" }
  | { readonly page: "project"; readonly name: string }
  | { readonly page: "agents" }
  | { readonly page: "agent"; readonly agent: string }
  | { readonly page: "machines" }
  | { readonly page: "machine"; readonly id: string }
  | { readonly page: "services"; readonly params: string }
  /** An empty action is the list of fleet actions. */
  | { readonly page: "fleet"; readonly action: string }
  | { readonly page: "console" }
  | { readonly page: "settings" }
  | { readonly page: "records"; readonly params: string }
  | { readonly page: "record"; readonly id: string; readonly node: string; readonly back: string }
  | {
      readonly page: "chat";
      readonly id: string;
      readonly node: string;
      readonly message: string;
      readonly shared: boolean;
    }
  | { readonly page: "missing" };

const plain = {
  "#/history": { page: "history" },
  "#/projects": { page: "projects" },
  "#/agents": { page: "agents" },
  "#/machines": { page: "machines" },
  "#/console": { page: "console" },
  "#/settings": { page: "settings" },
  "#/fleet": { page: "fleet", action: "" },
  // Earlier dashboards linked these; they now live on other pages.
  "#/status": { page: "services", params: "" },
  "#/continue": { page: "history" },
  "#/recent": { page: "search", params: "" },
} as const satisfies Record<string, Route>;

function query(hash: string): URLSearchParams {
  return new URLSearchParams(hash.split("?")[1]);
}
function segment(hash: string, prefix: string): string | undefined {
  if (!hash.startsWith(prefix)) return undefined;
  const value = hash.slice(prefix.length).split(/[?#]/)[0];
  return value ? decodeURIComponent(value) : undefined;
}

export function parseRoute(hash: string): Route {
  if (hash === "" || hash === "#" || hash === "#/") return { page: "overview" };
  try {
    if (hash in plain) return plain[hash as keyof typeof plain];
    if (/^#\/recent\?/.test(hash)) return { page: "search", params: "" };
    for (const page of ["services", "search", "records", "export"] as const)
      if (new RegExp(`^#/${page}(?:\\?|$)`).test(hash))
        return { page, params: query(hash).toString() };
    if (/^#\/project\?/.test(hash)) {
      const name = query(hash).get("name");
      return name === null ? { page: "missing" } : { page: "project", name };
    }
    const agent = segment(hash, "#/agent/");
    if (agent) return { page: "agent", agent };
    const machine = segment(hash, "#/machine/");
    if (machine) return { page: "machine", id: machine };
    const action = segment(hash, "#/fleet/");
    if (action) return { page: "fleet", action };
    const params = query(hash);
    const record = segment(hash, "#/record/");
    if (record) {
      const back = params.get("back") ?? "";
      return {
        page: "record",
        id: record,
        node: params.get("node") ?? "",
        back: /^#\/records(?:\?|$)/.test(back) ? back : "#/records",
      };
    }
    const chat = segment(hash, "#/chat/");
    if (chat)
      return {
        page: "chat",
        id: chat,
        node: params.get("node") ?? "",
        message: params.get("message") ?? "",
        shared: params.get("shared") === "true",
      };
  } catch {
    // A malformed escape in a pasted link is an unknown page, not a crash.
  }
  return { page: "missing" };
}

export function chatHref(id: string, node = "", message = "", shared = false): string {
  return `#/chat/${encodeURIComponent(id)}?${new URLSearchParams({ node, message, ...(shared ? { shared: "true" } : {}) })}`;
}
export function searchHref(filters: Record<string, string> = {}): string {
  const params = new URLSearchParams(filters);
  return params.size ? `#/search?${params}` : "#/search";
}
export function projectHref(name: string): string {
  return `#/project?${new URLSearchParams({ name })}`;
}
export function agentHref(agent: string): string {
  return `#/agent/${encodeURIComponent(agent)}`;
}
export function machineHref(id: string): string {
  return `#/machine/${encodeURIComponent(id)}`;
}
export function fleetHref(action = ""): string {
  return action ? `#/fleet/${encodeURIComponent(action)}` : "#/fleet";
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
    const update = () => {
      this.current = parseRoute(location.hash);
    };
    globalThis.addEventListener?.("mash:navigate", update);
    globalThis.addEventListener?.("popstate", update);
    globalThis.addEventListener?.("hashchange", update);
  }
}

export const router = new Router();
