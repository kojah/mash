import { describe, expect, it } from "vitest";
import { chatHref, parseRoute } from "./route.svelte";

describe("routes", () => {
  it("round-trips chat ids, including ones that need escaping", () => {
    for (const id of ["backfill", "a b/c?d"]) {
      expect(parseRoute(chatHref(id))).toEqual({ page: "chat", id });
    }
  });

  it("falls back to home for anything else", () => {
    for (const hash of ["", "#", "#/", "#/chat/", "#/elsewhere"]) {
      expect(parseRoute(hash)).toEqual({ page: "home" });
    }
  });
});
