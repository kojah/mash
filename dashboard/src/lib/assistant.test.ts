import { afterEach, describe, expect, it } from "vitest";
import { type AssistantEvent, ask, linkify, parseEvent, sources } from "./assistant";
import { install } from "./transport";

afterEach(() => install((path, init) => fetch(path, init)));

/** A response whose body arrives in the given pieces. */
function streamed(pieces: readonly string[], status = 200): Response {
  const encoder = new TextEncoder();
  const body = new ReadableStream<Uint8Array>({
    start(controller) {
      for (const piece of pieces) controller.enqueue(encoder.encode(piece));
      controller.close();
    },
  });
  return new Response(body, { status, headers: { "Content-Type": "text/event-stream" } });
}

describe("parseEvent", () => {
  it("reads each kind of event", () => {
    expect(parseEvent('{"type":"searching"}')).toEqual({ type: "searching" });
    expect(parseEvent('{"type":"text","delta":"hi"}')).toEqual({ type: "text", delta: "hi" });
    expect(parseEvent('{"type":"done","partial":true}')).toEqual({ type: "done", partial: true });
    expect(parseEvent('{"type":"failed","reason":"refused","message":"No."}')).toEqual({
      type: "failed",
      reason: "refused",
      message: "No.",
    });
  });
  it("turns down anything else", () => {
    for (const data of [
      "",
      "[DONE]",
      "null",
      '{"type":"text"}',
      '{"type":"done"}',
      '{"type":"failed","reason":"hungry","message":"x"}',
      '{"type":"run","command":"rm"}',
    ])
      expect(parseEvent(data)).toBeUndefined();
  });
});

describe("ask", () => {
  it("sends the conversation and hears events split across chunks", async () => {
    let sent: unknown;
    install((path, init) => {
      expect(path).toBe("/api/assistant");
      expect(init.method).toBe("POST");
      sent = JSON.parse(String(init.body));
      return Promise.resolve(
        streamed([
          ': keep-alive\n\ndata: {"type":"sear',
          'ching"}\n\ndata: {"type":"text","delta":"Port "}\n',
          '\ndata: {"type":"text","delta":"8080."}\n\ndata: <nonsense>\n\n',
          'data: {"type":"done","partial":false}\n\n',
        ]),
      );
    });
    const heard: AssistantEvent[] = [];
    await ask(
      [{ role: "user", content: "which port?" }],
      { id: "abc", title: "Ports" },
      new AbortController().signal,
      (event) => heard.push(event),
    );
    expect(sent).toEqual({
      messages: [{ role: "user", content: "which port?" }],
      open_chat: { id: "abc", title: "Ports" },
    });
    expect(heard).toEqual([
      { type: "searching" },
      { type: "text", delta: "Port " },
      { type: "text", delta: "8080." },
      { type: "done", partial: false },
    ]);
  });
  it("reports why the dashboard turned a question down", async () => {
    install(() =>
      Promise.resolve(
        new Response(JSON.stringify({ error: { message: "no assistant is configured" } }), {
          status: 409,
        }),
      ),
    );
    await expect(
      ask([{ role: "user", content: "hi" }], undefined, new AbortController().signal, () => {}),
    ).rejects.toThrow("no assistant is configured");
  });
});

describe("links in an answer", () => {
  const answer =
    "See [Port decision](chat:abc-123) and [the laptop one](chat:x%20y?node=laptop). " +
    "Saved as [use 8080](memory:rec-1). Again: [Port decision](chat:abc-123). " +
    "Not ours: [elsewhere](https://example.com) and [odd](javascript:alert(1)).";
  it("become the dashboard's own addresses", () => {
    const text = linkify(answer);
    expect(text).toContain("[Port decision](#/chat/abc-123?node=&message=)");
    expect(text).toContain("[the laptop one](#/chat/x%20y?node=laptop&message=&shared=true)");
    expect(text).toContain("[use 8080](#/record/rec-1?");
    // Anything else is left as written, for the sanitizer to judge.
    expect(text).toContain("(https://example.com)");
    expect(text).toContain("(javascript:alert(1))");
  });
  it("are listed once each as sources", () => {
    expect(sources(answer).map((source) => [source.kind, source.title])).toEqual([
      ["chat", "Port decision"],
      ["chat", "the laptop one"],
      ["memory", "use 8080"],
    ]);
    expect(sources("Nothing found.")).toEqual([]);
  });
});
