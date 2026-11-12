import { describe, expect, it } from "vitest";
import { parseCatalog, parseMessages, parseSearch } from "../lib/api";
import { parseConsole } from "../lib/console";
import { parseRecord, parseRecordPage } from "../lib/records";
import { parseFleetCatalog, parseMachines, parseSetup } from "../lib/services.svelte";
import { parseSettings } from "../lib/settings";
import { parseStatus } from "../lib/status";
import { type Reply, respond, terms } from "./backend";
import { chats, localMachine, records } from "./history";

const now = Date.UTC(2026, 10, 12);
function get(path: string): Reply {
  return respond(path, "GET", now);
}
function body(path: string): unknown {
  const answer = get(path);
  expect(answer.status, path).toBe(200);
  // What the browser would receive: only what survives as JSON.
  return JSON.parse(JSON.stringify(answer.body));
}

describe("the demo answers in the shapes mash dash sends", () => {
  it("lists every chat, on more than one machine and agent", () => {
    const catalog = parseCatalog(body("/api/chats"));
    expect(catalog.localNode).toBe(localMachine);
    expect(catalog.chats).toHaveLength(chats.length);
    expect(new Set(catalog.chats.map((chat) => chat.machine)).size).toBeGreaterThan(1);
    expect(new Set(catalog.chats.map((chat) => chat.agent)).size).toBe(6);
    expect(new Set(catalog.chats.map((chat) => chat.id)).size).toBe(chats.length);
  });

  it("serves each transcript, with tool calls joined to their output", () => {
    for (const chat of chats) {
      const page = parseMessages(body(`/api/chat?id=${chat.id}&offset=0&limit=100`));
      expect(page.messages).toHaveLength(chat.messages.length);
      const calls = page.messages.flatMap((message) =>
        (message.tools?.calls ?? []).map((c) => c.callId),
      );
      const results = page.messages.flatMap((message) =>
        (message.tools?.results ?? []).map((r) => r.callId),
      );
      expect(results).toEqual(calls);
    }
    expect(get("/api/chat?id=missing&offset=0").status).toBe(404);
  });

  it("opens the conversation around a message", () => {
    const page = parseMessages(body("/api/view?id=sign-in-tokens-2&context=25"));
    expect(page.messages.map((message) => message.id)).toContain("sign-in-tokens-2");
  });

  it("finds chats by the words of a question, best first", () => {
    const page = parseSearch(
      body(`/api/search?${new URLSearchParams({ q: "What did we decide about sign-in tokens?" })}`),
      localMachine,
    );
    expect(page.hits[0]?.chat.id).toBe("sign-in-tokens");
    expect(page.hits[0]?.messageId).toMatch(/^sign-in-tokens-\d+$/);
    expect(page.hits[0]?.before.toLowerCase()).toContain("token");
    expect(page.warnings.join(" ")).toContain("demo");
  });

  it("applies the search filters", () => {
    const filtered = parseSearch(
      body(
        `/api/search?${new URLSearchParams({ q: "queue for exports", machine: "workstation" })}`,
      ),
      localMachine,
    );
    expect(filtered.hits.length).toBeGreaterThan(0);
    expect(filtered.hits.every((hit) => hit.chat.machine === "workstation")).toBe(true);
    const recent = parseSearch(
      body(`/api/search?${new URLSearchParams({ q: "checkout", days: "7" })}`),
      localMachine,
    );
    expect(recent.hits.map((hit) => hit.chat.id)).toEqual(["flaky-checkout-test"]);
    expect(parseSearch(body("/api/search?q=zebra"), localMachine).hits).toEqual([]);
  });

  it("drops common words and endings from a question", () => {
    expect(terms("What did we decide about the tokens?")).toEqual(["decide", "token"]);
  });

  it("lists memories that quote a message which exists", () => {
    const page = parseRecordPage(body("/api/records"));
    expect(page.records).toHaveLength(records.length);
    for (const entry of records) {
      const chat = chats.find((candidate) => candidate.id === entry.source.chat);
      expect(chat?.messages[entry.source.message]?.content, entry.id).toContain(entry.source.quote);
      expect(parseRecord(body(`/api/record?id=${entry.id}&link_offset=0`)).id).toBe(entry.id);
    }
    expect(parseRecordPage(body("/api/records?kind=decision")).records).toHaveLength(
      records.filter((entry) => entry.kind === "decision").length,
    );
  });

  it("reports status, settings, machines and tools", () => {
    expect(parseStatus(body("/api/status")).ready).toBe(true);
    expect(parseSettings(body("/api/services/settings")).editable).toBe(false);
    expect(parseMachines(body("/api/services")).map((machine) => machine.id)).toEqual([
      localMachine,
    ]);
    expect(parseSetup(body("/api/services/installation?machine=laptop")).agents).toHaveLength(2);
    expect(parseFleetCatalog(body("/api/services/fleet")).enabled).toBe(false);
    expect(parseConsole(body("/api/console/tools")).tools.length).toBeGreaterThan(0);
  });

  it("refuses to change or run anything", () => {
    expect(respond("/api/services/settings", "POST", now).status).toBe(403);
    expect(get("/api/console/call?name=search").status).toBe(403);
    expect(get("/api/elsewhere").status).toBe(404);
  });
});
