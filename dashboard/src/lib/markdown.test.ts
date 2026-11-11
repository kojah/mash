import { describe, expect, it } from "vitest";
import { plainTitle } from "./markdown";

describe("chat titles", () => {
  it("drops emphasis and code marks but keeps the words and other punctuation", () => {
    expect(plainTitle("Master is at **1a7a4d6e1**, see `docs/plan.md`")).toBe(
      "Master is at 1a7a4d6e1, see docs/plan.md",
    );
    expect(plainTitle("2 * 3 and __init__")).toBe("2 * 3 and __init__");
  });
});
