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
        return { messages: offset < 200 ? [message(String(offset))] : [] };
      },
    );
    expect(offsets).toEqual([0, 100, 200]);
    expect(result.map((row) => row.id)).toEqual(["0", "100"]);
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
