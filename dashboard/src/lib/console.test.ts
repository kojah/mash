import { describe, expect, it } from "vitest";
import { argumentsFor, headline, locked, parseAnswer, parseConsole, summarize } from "./console";

const listing = {
  build_revision: "abc123",
  contract_fingerprint: "f00d",
  tools: [
    {
      name: "search",
      description: "Find chats by what was said. It never indexes.",
      annotations: { readOnlyHint: true, destructiveHint: false },
      inputSchema: {
        type: "object",
        required: ["query"],
        $defs: {
          Detail: { type: "string", enum: ["compact", "extended"], description: "How much." },
        },
        properties: {
          days: { type: ["integer", "null"], default: null, minimum: 0 },
          detail: { $ref: "#/$defs/Detail", default: "compact" },
          chat: { type: "array", items: { type: "string" }, default: [], description: "Chat IDs." },
          intent: { anyOf: [{ type: "string", enum: ["exact", "related"] }, { type: "null" }] },
          shared: { type: "boolean", default: false },
          query: { type: "string", description: "Words to look for." },
        },
      },
    },
    {
      name: "forget",
      description: "Remove a chat.",
      annotations: { readOnlyHint: false },
      inputSchema: {},
    },
    { name: "brand_new", description: "", annotations: { readOnlyHint: true } },
  ],
};

describe("console tool listing", () => {
  const console = parseConsole(listing);
  const search = console.tools[0];
  if (!search) throw new Error("search");
  const named = (name: string) => search.params.find((entry) => entry.name === name);

  it("reads types, required marks, choices and defaults from each tool's schema", () => {
    expect(console.build).toBe("abc123");
    expect(search.params[0]).toMatchObject({ name: "query", required: true, kind: "string" });
    expect(named("days")).toMatchObject({ kind: "integer", type: "integer", fallback: "" });
    expect(named("detail")).toMatchObject({
      kind: "enum",
      choices: ["compact", "extended"],
      about: "How much.",
      fallback: '"compact"',
    });
    expect(named("intent")).toMatchObject({ kind: "enum", choices: ["exact", "related"] });
    expect(named("chat")).toMatchObject({ kind: "json", type: "array", about: "Chat IDs." });
    expect(named("shared")).toMatchObject({ kind: "boolean", fallback: "false" });
  });

  it("groups by a short hand-kept list, locks writers and keeps unknown tools visible", () => {
    // Listed tools come first in their hand-kept order, the rest by name.
    expect(console.tools.map((tool) => [tool.name, tool.group])).toEqual([
      ["search", "History"],
      ["brand_new", "Other"],
      ["forget", locked],
    ]);
    expect(headline(search.about)).toBe("Find chats by what was said.");
    expect(headline("no full stop")).toBe("no full stop");
  });

  it("turns a filled form into typed arguments, leaving blanks to the tool's defaults", () => {
    const fill = (values: Record<string, string>) =>
      argumentsFor(
        search,
        Object.fromEntries(Object.entries(values).map(([key, value]) => [`search.${key}`, value])),
      );
    expect(
      fill({ query: " watcher ", days: "7", shared: "true", chat: '["a"]', detail: "" }),
    ).toEqual({
      ok: true,
      arguments: { query: "watcher", days: 7, shared: true, chat: ["a"] },
    });
    expect(fill({})).toMatchObject({ ok: false, param: "query" });
    expect(fill({ query: "x", days: "soon" })).toMatchObject({ ok: false, param: "days" });
    expect(fill({ query: "x", chat: "a, b" })).toMatchObject({ ok: false, param: "chat" });
  });
});

describe("console answers", () => {
  it("summarises success by the size of the reply's first list", () => {
    const answer = parseAnswer({
      failed: false,
      reply: { local: "x", chats: [1, 2, 3] },
      text: "",
    });
    expect(summarize(answer)).toEqual({ state: "ok", heading: "", items: 3 });
    expect(
      summarize(parseAnswer({ failed: false, reply: { ready: true }, text: "" })).items,
    ).toBeNull();
  });

  it("uses the error the reply carries, and calls a retryable one busy", () => {
    const error = (retryable: boolean) =>
      parseAnswer({
        failed: true,
        reply: { error: { kind: "index-busy", message: "being updated", retryable } },
        text: "ignored",
      });
    expect(summarize(error(true))).toEqual({
      state: "busy",
      heading: "being updated",
      items: null,
    });
    expect(summarize(error(false)).state).toBe("failed");
    expect(
      summarize(parseAnswer({ failed: true, reply: null, text: "plain refusal\nmore" })).heading,
    ).toBe("plain refusal");
    expect(() => parseAnswer({ reply: {} })).toThrow();
  });
});
