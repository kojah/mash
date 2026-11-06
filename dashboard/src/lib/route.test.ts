import { describe, expect, it } from "vitest";
import { chatHref, parseRoute } from "./route.svelte";

describe("routes", () => {
  it("round-trips chat ids, including ones that need escaping", () => {
    for (const id of ["backfill", "a b/c?d"]) {
      expect(parseRoute(chatHref(id, "machine & one", "message/2"))).toEqual({
        page: "chat",
        id,
        node: "machine & one",
        message: "message/2",
      });
    }
  });

  it("falls back to home for anything else", () => {
    for (const hash of ["", "#", "#/", "#/chat/", "#/elsewhere", "#/chat/%ZZ"]) {
      expect(parseRoute(hash)).toEqual({ page: "home" });
    }
  });
});
