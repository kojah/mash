import { describe, expect, it } from "vitest";
import { pageOf } from "./pagination";

const items = Array.from({ length: 120 }, (_, index) => index);

describe("pageOf", () => {
  it("returns one page and its one-based range", () => {
    expect(pageOf(items, 2, 50)).toMatchObject({
      page: 2,
      pageCount: 3,
      first: 51,
      last: 100,
      total: 120,
    });
    expect(pageOf(items, 3, 50).items).toEqual(items.slice(100));
  });

  it("moves out-of-range pages back into the list", () => {
    expect(pageOf(items, 99, 50).page).toBe(3);
    expect(pageOf(items, 0, 50).page).toBe(1);
  });

  it("handles an empty list", () => {
    expect(pageOf([], 4, 50)).toEqual({
      items: [],
      page: 1,
      pageCount: 1,
      first: 0,
      last: 0,
      total: 0,
    });
  });
});
