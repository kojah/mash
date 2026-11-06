import { describe, expect, it } from "vitest";
import { browse, parseCatalog, parseMessages, parseSearch } from "./api";

const row = {
  chat_id: "same-id",
  provider: "codex",
  title: "<script>untrusted</script>",
  project: "/project",
  updated_at: 1_000_000_000,
};
describe("dashboard API boundary", () => {
  it("opens the evidence source from lookup even when a chat's representative differs", () => {
    const page = parseSearch(
      {
        results: [
          {
            ...row,
            node_id: "representative",
            matches: [
              {
                message_id: "raw",
                snippet: "match",
                lookup: { view: { id: "anchor", shared: "evidence-node" } },
              },
            ],
          },
        ],
      },
      "local",
    );
    expect(page.hits[0]?.chat.machine).toBe("evidence-node");
    expect(page.hits[0]?.messageId).toBe("anchor");
  });
  it("preserves machine identities, timestamps and partial-result warnings", () => {
    const catalog = parseCatalog({
      local_node: "local",
      truncated: true,
      shared_error: "unreachable",
      chats: [
        { ...row, node_id: "local" },
        { ...row, node_id: "remote" },
      ],
    });
    expect(catalog.chats.map((chat) => chat.machine)).toEqual(["local", "remote"]);
    expect(catalog.chats[0]?.title).toBe(row.title);
    expect(catalog.chats[0]?.updatedMs).toBe(row.updated_at);
    expect(catalog.chats[0]?.messageCount).toBeUndefined();
    expect(catalog.warnings).toHaveLength(2);
  });
  it("applies project, machine, agent and date filters to empty searches", () => {
    const catalog = parseCatalog({
      local_node: "local",
      truncated: false,
      chats: [
        row,
        { ...row, chat_id: "old", updated_at: 0 },
        { ...row, chat_id: "other", project: "/other" },
      ],
    });
    const filters = {
      agents: ["codex" as const],
      machines: ["local"],
      projects: ["/project"],
      days: 1,
    };
    expect(browse(catalog, filters, row.updated_at).hits.map((hit) => hit.chat.id)).toEqual([
      "same-id",
    ]);
    expect(browse(catalog, { ...filters, machines: ["remote"] }, row.updated_at).hits).toEqual([]);
  });
  it("rejects malformed records rather than presenting an empty success", () => {
    expect(() =>
      parseCatalog({
        local_node: "local",
        truncated: false,
        chats: [{ ...row, provider: "unknown" }],
      }),
    ).toThrow("Unsupported agent");
    expect(() => parseMessages({ messages: [{ id: "a", ordinal: "0" }] })).toThrow();
    expect(() => parseCatalog({ chats: [] })).toThrow();
  });
  it("retains provider roles, tool input and raw message text for safe rendering", () => {
    const page = parseMessages({
      messages: [
        {
          id: "a",
          ordinal: 37,
          role: "tool",
          content: "<img src=x onerror=alert(1)>",
          tools: { tool_calls: [{ name: "run", input: { command: "echo hello" } }] },
        },
      ],
    });
    expect(page.messages[0]?.role).toBe("tool");
    expect(page.messages[0]?.ordinal).toBe(37);
    expect(page.messages[0]?.activity).toContain("echo hello");
    expect(page.messages[0]?.blocks).toEqual([
      { type: "text", markdown: "<img src=x onerror=alert(1)>" },
    ]);
  });
});
