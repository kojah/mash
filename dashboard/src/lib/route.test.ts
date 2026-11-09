import { describe, expect, it } from "vitest";
import { chatHref, parseRoute, recordHref, recordsHref } from "./route.svelte";

describe("routes", () => {
  it("round-trips record identities and filtered return routes without external back links", () => {
    const back = recordsHref(
      new URLSearchParams({
        q: "port & timeout",
        kind: "fact",
        shared: "true",
        cursor_node: "node/a",
        cursor_offset: "20",
      }),
    );
    expect(parseRoute(back)).toEqual({
      page: "records",
      params: back.slice(back.indexOf("?") + 1),
    });
    expect(parseRoute(recordHref("id/one?two", "node & one", back))).toEqual({
      page: "record",
      id: "id/one?two",
      node: "node & one",
      back,
    });
    expect(parseRoute(recordHref("id", "", "javascript:alert(1)"))).toEqual({
      page: "record",
      id: "id",
      node: "",
      back: "#/records",
    });
    expect(parseRoute("#/record/%ZZ")).toEqual({ page: "home" });
  });
  it("round-trips chat ids, including ones that need escaping", () => {
    for (const id of ["backfill", "a b/c?d"]) {
      expect(parseRoute(chatHref(id, "machine & one", "message/2"))).toEqual({
        page: "chat",
        id,
        node: "machine & one",
        message: "message/2",
        shared: false,
      });
    }
  });
  it("retains explicit shared source identity even for this machine's node", () => {
    expect(parseRoute(chatHref("chat", "local", "source", true))).toEqual({
      page: "chat",
      id: "chat",
      node: "local",
      message: "source",
      shared: true,
    });
  });

  it("falls back to home for anything else", () => {
    for (const hash of ["", "#", "#/", "#/chat/", "#/elsewhere", "#/chat/%ZZ"]) {
      expect(parseRoute(hash)).toEqual({ page: "home" });
    }
  });
});

describe("recent conversation pages", () => {
  it("reads the page from the route and ignores invalid ones", () => {
    expect(parseRoute("#/recent?page=3")).toEqual({
      page: "browse",
      section: "recent",
      listPage: 3,
    });
    for (const value of ["0", "-1", "1.5", "x"]) {
      expect(parseRoute(`#/recent?page=${value}`)).toMatchObject({ listPage: 1 });
    }
    expect(parseRoute("#/recent")).toEqual({ page: "browse", section: "recent" });
  });
});

it("sends the old index status route to services", () => {
  expect(parseRoute("#/status")).toEqual({ page: "services", params: "" });
});
