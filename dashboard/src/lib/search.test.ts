import { describe, expect, it } from "vitest";
import { appliedHref, parseApplied, sameSearch } from "./search";

describe("applied searches", () => {
  it("round-trips a search through its address", () => {
    const applied = parseApplied(
      "q=qdrant+filter&agent=codex,claude-code&machine=xenia&days=7&offset=40",
    );
    expect(applied).toEqual({
      query: "qdrant filter",
      agents: ["codex", "claude-code"],
      machines: ["xenia"],
      projects: [],
      days: 7,
      sort: "new",
      rank: "relevance",
      offset: 40,
    });
    expect(parseApplied(appliedHref(applied).split("?")[1] ?? "")).toEqual(applied);
    expect(appliedHref(parseApplied(""))).toBe("#/search");
  });
  it("ignores values the controls cannot produce", () => {
    const applied = parseApplied("agent=shell,codex&days=13&sort=sideways&offset=-5");
    expect(applied.agents).toEqual(["codex"]);
    expect(applied.days).toBeNull();
    expect(applied.sort).toBe("new");
    expect(parseApplied("rank=sideways").rank).toBe("relevance");
    expect(parseApplied("q=x&rank=recent").rank).toBe("recent");
    expect(applied.offset).toBe(0);
  });
  it("treats a page change as the same search and a filter change as a new one", () => {
    const first = parseApplied("q=tabs&machine=xenia");
    expect(sameSearch(first, { ...first, offset: 25 })).toBe(true);
    expect(sameSearch(first, { ...first, machines: [] })).toBe(false);
    expect(sameSearch(first, { ...first, sort: "old" })).toBe(false);
    expect(sameSearch(first, { ...first, rank: "recent" })).toBe(false);
  });
});
