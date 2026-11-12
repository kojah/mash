import { describe, expect, it } from "vitest";
import { chatKey, chatPath, exportChats, jsonExport } from "./bulkExport";
import type { ChatSummary, Message } from "./types";
import { crc32 } from "./zip";

const chat = (id: string, patch: Partial<ChatSummary> = {}): ChatSummary => ({
  id,
  title: `Chat ${id}`,
  agent: "codex",
  machine: "xenia",
  project: "/home/me/mash",
  updatedAt: "",
  updatedMs: Date.UTC(2026, 10, 12, 5),
  ...patch,
});
const message = (id: string, text: string): Message => ({
  id,
  role: "user",
  at: "Now",
  blocks: [{ type: "text", markdown: text }],
});

/** Reads a stored ZIP back: every name with its text, checking each checksum. */
async function unzip(blob: Blob): Promise<Map<string, string>> {
  const bytes = new Uint8Array(await blob.arrayBuffer());
  const view = new DataView(bytes.buffer);
  const end = bytes.length - 22;
  expect(view.getUint32(end, true)).toBe(0x06054b50);
  const files = new Map<string, string>();
  let at = view.getUint32(end + 16, true);
  for (let index = 0; index < view.getUint16(end + 10, true); index += 1) {
    expect(view.getUint32(at, true)).toBe(0x02014b50);
    const size = view.getUint32(at + 24, true);
    const nameLength = view.getUint16(at + 28, true);
    const local = view.getUint32(at + 42, true);
    const name = new TextDecoder().decode(bytes.subarray(at + 46, at + 46 + nameLength));
    expect(view.getUint32(local, true)).toBe(0x04034b50);
    const start = local + 30 + view.getUint16(local + 26, true);
    const content = bytes.subarray(start, start + size);
    expect(crc32(content)).toBe(view.getUint32(at + 16, true));
    files.set(name, new TextDecoder().decode(content));
    at += 46 + nameLength;
  }
  return files;
}

describe("bulk chat export", () => {
  it("matches the standard checksum", () => {
    expect(crc32(new TextEncoder().encode("123456789"))).toBe(0xcbf43926);
  });

  it("files chats by project and day, keeping unsafe and repeated titles apart", () => {
    const taken = new Set<string>();
    const { updatedMs: _, ...undated } = chat("c");
    expect(chatPath(chat("a", { title: "fix: a/b **now**?" }), "markdown", taken)).toBe(
      "mash/2026-11-12 fix a b now.md",
    );
    expect(chatPath(chat("b", { title: "Fix: A/B now" }), "markdown", taken)).toBe(
      "mash/2026-11-12 Fix A B now (2).md",
    );
    expect(chatPath({ ...undated, title: "..", project: "" }, "json", taken)).toBe(
      "no project/undated chat.json",
    );
  });

  it("tells the same chat id on two machines apart", () => {
    expect(chatKey(chat("a"))).not.toBe(chatKey(chat("a", { machine: "ares" })));
  });

  it("packs one readable file per chat, in the order given", async () => {
    const seen: string[] = [];
    const progress: number[] = [];
    const blob = await exportChats(
      [chat("a"), chat("b", { machine: "ares", project: "C:\\work\\disk-report" })],
      "markdown",
      new AbortController().signal,
      (state) => progress.push(state.chats),
      async (id, node, offset) => {
        if (offset === 0) seen.push(`${node}/${id}`);
        return { messages: offset === 0 ? [message(`${id}-1`, `héllo from ${id}`)] : [] };
      },
      new Date(2026, 10, 12, 5, 0, 0),
    );
    expect(seen.sort()).toEqual(["ares/b", "xenia/a"]);
    expect(progress.at(-1)).toBe(2);
    const files = await unzip(blob);
    expect([...files.keys()]).toEqual([
      "mash/2026-11-12 Chat a.md",
      "disk-report/2026-11-12 Chat b.md",
    ]);
    expect(files.get("mash/2026-11-12 Chat a.md")).toContain("héllo from a");
    expect(files.get("disk-report/2026-11-12 Chat b.md")).toContain("Machine: ares");
  });

  it("writes JSON a script can read back", () => {
    const parsed = JSON.parse(jsonExport(chat("a"), [message("m1", "hi")]));
    expect(parsed).toMatchObject({
      id: "a",
      agent: "codex",
      machine: "xenia",
      updated: "2026-11-12T05:00:00.000Z",
      messages: [{ id: "m1", role: "user", kind: "dialogue", text: "hi", tools: null }],
    });
  });

  it("saves nothing when one chat cannot be read, and names it", async () => {
    await expect(
      exportChats(
        [chat("a"), chat("gone", { title: "Lost chat" })],
        "json",
        new AbortController().signal,
        () => {},
        async (id, _node, offset) => {
          if (id === "gone") throw new Error("That chat is no longer in the index.");
          return { messages: offset === 0 ? [message("m", "x")] : [] };
        },
      ),
    ).rejects.toThrow(/Lost chat.*no longer in the index.*Nothing was saved/);
  });

  it("stops when asked", async () => {
    const controller = new AbortController();
    const running = exportChats(
      [chat("a")],
      "json",
      controller.signal,
      () => {},
      async () => {
        controller.abort();
        return { messages: [message("m", "x")] };
      },
    );
    await expect(running).rejects.toThrow();
  });
});
