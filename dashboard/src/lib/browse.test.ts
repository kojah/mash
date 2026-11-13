import { describe, expect, it } from "vitest";
import { arrangeProjects, groupChats, projectLabel, whenLabel } from "./browse";
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
    expect(whenLabel(now - 20_000, now)).toBe("now");
    expect(whenLabel(now + 5_000, now)).toBe("now");
    expect(whenLabel(now - 3 * 60_000, now)).toBe("3m");
    expect(whenLabel(now - 3 * 3_600_000, now)).toBe("3h");
    expect(whenLabel(now - 3 * 86_400_000, now)).toBe("3d");
    expect(whenLabel(now - 30 * 86_400_000, now)).toBe("30d");
    expect(whenLabel(now - 45 * 86_400_000, now)).not.toMatch(/^\d+d$/);
    expect(whenLabel(new Date(2025, 2, 4).getTime(), now)).toContain("2025");
  });
});

describe("projects view", () => {
  const projects = groupChats(
    [
      chat("a", { project: "/work/mash", updatedMs: 300 }),
      chat("b", { project: "/work/mash", agent: "claude-code", machine: "mac", updatedMs: 200 }),
      chat("c", { project: "/work/alpha", updatedMs: 900 }),
      chat("d", { project: "/work/zeta", agent: "claude-code", updatedMs: 500 }),
    ],
    (entry) => entry.project,
  );
  const names = (view: Partial<Parameters<typeof arrangeProjects>[1]>) =>
    arrangeProjects(projects, { find: "", agent: "", machine: "", sort: "chats", ...view }).map(
      (project) => projectLabel(project.key),
    );

  it("orders by size, by latest chat, or by name", () => {
    expect(names({})).toEqual(["mash", "alpha", "zeta"]);
    expect(names({ sort: "recent" })).toEqual(["alpha", "zeta", "mash"]);
    expect(names({ sort: "name" })).toEqual(["alpha", "mash", "zeta"]);
  });

  it("keeps projects that match the name and have a chat from the agent and machine", () => {
    expect(names({ find: " MA " })).toEqual(["mash"]);
    expect(names({ agent: "claude-code" })).toEqual(["mash", "zeta"]);
    expect(names({ machine: "mac" })).toEqual(["mash"]);
    expect(names({ agent: "claude-code", machine: "mac", find: "zeta" })).toEqual([]);
  });
});
