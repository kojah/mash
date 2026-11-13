import { describe, expect, it } from "vitest";
import { countLabel, findMatches } from "./find";

const chat = [
  { id: "a", text: "Which Queue should we use?" },
  { id: "b", text: "A queue fits.\nEach queue\nworker runs alone." },
  { id: "c", text: "Ship it." },
];

describe("find in chat", () => {
  it("lists every occurrence in reading order, ignoring case", () => {
    expect(findMatches(chat, "queue")).toEqual([
      { messageId: "a", position: 0, occurrence: 0 },
      { messageId: "b", position: 1, occurrence: 0 },
      { messageId: "b", position: 1, occurrence: 1 },
    ]);
  });
  it("matches a phrase across a line break", () => {
    expect(findMatches(chat, "queue worker")).toEqual([
      { messageId: "b", position: 1, occurrence: 0 },
    ]);
  });
  it("finds nothing for an empty or unmatched query", () => {
    expect(findMatches(chat, "   ")).toEqual([]);
    expect(findMatches(chat, "kafka")).toEqual([]);
  });
  it("stops at the limit and says so in the count", () => {
    expect(findMatches(chat, "e", 2)).toHaveLength(2);
    expect(countLabel(0, 2, 2)).toBe("1 of 2+");
    expect(countLabel(2, 17)).toBe("3 of 17");
    expect(countLabel(0, 0)).toBe("No matches");
  });
});
