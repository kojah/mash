import { describe, expect, it } from "vitest";
import {
  agentHref,
  chatHref,
  fleetHref,
  machineHref,
  parseRoute,
  projectHref,
  recordHref,
  recordsHref,
  searchHref,
} from "./route.svelte";

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
    expect(parseRoute("#/record/%ZZ")).toEqual({ page: "missing" });
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

  it("opens the overview with no route and reports unknown ones", () => {
    for (const hash of ["", "#", "#/"]) expect(parseRoute(hash)).toEqual({ page: "overview" });
    for (const hash of ["#/chat/", "#/elsewhere", "#/chat/%ZZ", "#/project", "#/agent/"])
      expect(parseRoute(hash)).toEqual({ page: "missing" });
  });
});

describe("browse pages", () => {
  it("round-trips names that need escaping", () => {
    expect(parseRoute(projectHref("/home/me/a b & c"))).toEqual({
      page: "project",
      name: "/home/me/a b & c",
    });
    expect(parseRoute(machineHref("dev mac/1"))).toEqual({ page: "machine", id: "dev mac/1" });
    expect(parseRoute(agentHref("claude-code"))).toEqual({ page: "agent", agent: "claude-code" });
    expect(parseRoute(fleetHref())).toEqual({ page: "fleet", action: "" });
    expect(parseRoute(fleetHref("watch-start"))).toEqual({ page: "fleet", action: "watch-start" });
  });
  it("keeps a search in its address", () => {
    expect(parseRoute(searchHref())).toEqual({ page: "search", params: "" });
    expect(parseRoute(searchHref({ q: "port & timeout", machine: "xenia" }))).toEqual({
      page: "search",
      params: "q=port+%26+timeout&machine=xenia",
    });
  });
  it("sends earlier dashboards' list routes to the pages that replaced them", () => {
    expect(parseRoute("#/recent")).toEqual({ page: "search", params: "" });
    expect(parseRoute("#/recent?page=3")).toEqual({ page: "search", params: "" });
    expect(parseRoute("#/continue")).toEqual({ page: "history" });
    for (const page of ["projects", "agents", "machines", "console", "settings", "history"])
      expect(parseRoute(`#/${page}`)).toEqual({ page });
  });
});

it("sends the old index status route to services", () => {
  expect(parseRoute("#/status")).toEqual({ page: "services", params: "" });
});
