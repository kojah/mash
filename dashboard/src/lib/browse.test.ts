import { describe, expect, it } from "vitest";
import { groupChats, projectLabel, whenLabel } from "./browse";
import type { ChatSummary } from "./types";

const chat = (id: string, change: Partial<ChatSummary>): ChatSummary => ({
  id,
  title: id,
  agent: "codex",
  machine: "xenia",
  project: "/work/mash",
  updatedAt: "",
  ...change,
});

describe("catalogue grouping", () => {
  const chats = [
    chat("a", { updatedMs: 300 }),
    chat("b", { agent: "claude-code", machine: "mac", updatedMs: 500 }),
    chat("c", { project: "/work/other", updatedMs: 100 }),
  ];
  it("groups by a field, largest first, with what else is true of each group", () => {
    const [first, second] = groupChats(chats, (item) => item.project);
    expect(first?.key).toBe("/work/mash");
    expect(first?.chats.map((item) => item.id)).toEqual(["a", "b"]);
    expect(first?.latestMs).toBe(500);
    expect(first?.agents).toEqual([
      ["codex", 1],
      ["claude-code", 1],
    ]);
    expect(first?.machines.map(([machine]) => machine)).toEqual(["xenia", "mac"]);
    expect(second?.key).toBe("/work/other");
  });
  it("labels a project by its folder and a time by how recent it is", () => {
    expect(projectLabel("/home/me/scratch/mash/")).toBe("mash");
    expect(projectLabel("C:\\work\\app")).toBe("app");
    expect(projectLabel("")).toBe("no project");
    const now = new Date(2026, 10, 10, 18, 0).getTime();
    expect(whenLabel(undefined, now)).toBe("time unknown");
    expect(whenLabel(new Date(2026, 10, 10, 9, 5).getTime(), now)).toMatch(/09[:.]05/);
    expect(whenLabel(new Date(2025, 2, 4).getTime(), now)).toContain("2025");
  });
});
