import { describe, expect, it } from "vitest";
import { collectChat, markdownExport } from "./export";
import type { Message } from "./types";

const message = (id: string): Message => ({
  id,
  role: "assistant",
  at: "Now",
  blocks: [{ type: "text", markdown: `Message ${id}` }],
});

describe("whole-chat exports", () => {
  it("reads beyond a short page and preserves source identity", async () => {
    const offsets: number[] = [];
    const result = await collectChat(
      "chat",
      "remote",
      new AbortController().signal,
      () => {},
      async (id, node, offset) => {
        expect([id, node]).toEqual(["chat", "remote"]);
        offsets.push(offset);
        return { messages: offset < 1000 ? [message(String(offset))] : [] };
      },
    );
    expect(offsets).toEqual([0, 500, 1000]);
    expect(result.map((row) => row.id)).toEqual(["0", "500"]);
  });

  it("asks for the rest of a long chat together, in order", async () => {
    const offsets: number[] = [];
    const limits = new Set<number>();
    const result = await collectChat(
      "chat",
      "local",
      new AbortController().signal,
      () => {},
      async (_id, _node, offset, _signal, limit) => {
        offsets.push(offset);
        limits.add(limit);
        // Later pages answer sooner than earlier ones.
        await new Promise((resolve) => setTimeout(resolve, 20 - offset / 100));
        return { total: 1450, messages: offset < 1450 ? [message(String(offset))] : [] };
      },
    );
    expect(offsets).toEqual([0, 500, 1000, 1500]);
    expect([...limits]).toEqual([500]);
    expect(result.map((row) => row.id)).toEqual(["0", "500", "1000"]);
  });

  it("rejects failed or unstable reads without returning partial data", async () => {
    await expect(
      collectChat(
        "chat",
        "local",
        new AbortController().signal,
        () => {},
        async () => ({ messages: [message("same")] }),
      ),
    ).rejects.toThrow("changed during export");
    await expect(
      collectChat(
        "chat",
        "local",
        new AbortController().signal,
        () => {},
        async (_id, _node, offset) => {
          if (offset) throw new Error("offline");
          return { messages: [message("one")] };
        },
      ),
    ).rejects.toThrow("offline");
  });

  it("cancels before requesting another page", async () => {
    const controller = new AbortController();
    let reads = 0;
    await expect(
      collectChat(
        "chat",
        "local",
        controller.signal,
        () => controller.abort(),
        async () => {
          reads++;
          return { messages: [message("one")] };
        },
      ),
    ).rejects.toThrow();
    expect(reads).toBe(1);
  });

  it("preserves Markdown and fences literal tool output containing code fences", () => {
    const output = markdownExport("Chat", "remote", [
      message("one"),
      {
        ...message("tool"),
        presentation: "tool-result",
        blocks: [{ type: "text", markdown: "```html\n<div>raw</div>\n```" }],
      },
    ]);
    expect(output).toContain("Machine: remote");
    expect(output).toContain("Message one");
    expect(output).toContain("## Tool output");
    expect(output).toContain("````\n```html\n<div>raw</div>\n```\n````");
  });
});
