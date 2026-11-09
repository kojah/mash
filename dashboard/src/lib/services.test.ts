import { describe, expect, it } from "vitest";
import { parseFleetCatalog, parseFleetJob, serviceRequestId } from "./services.svelte";

describe("fleet response validation", () => {
  const request = {
    id: "00000000-0000-4000-8000-000000000001",
    action: "sync",
    targets: ["xenia"],
    revision: "configured-revision",
  };
  const job = {
    request,
    requested_at: 1,
    completed_at: null,
    outcome: "running",
    targets: [{ id: "xenia", state: "running", progress: { progress: 2.5, total: 10 } }],
  };

  it("preserves uncertain outcomes and per-machine progress", () => {
    expect(parseFleetJob(job).targets[0]?.progress).toEqual({ progress: 2.5, total: 10 });
    expect(parseFleetJob({ ...job, outcome: "unknown" }).outcome).toBe("unknown");
  });

  it("rejects unsupported actions, outcomes and invalid progress", () => {
    expect(() => parseFleetJob({ ...job, request: { ...request, action: "shell" } })).toThrow();
    expect(() => parseFleetJob({ ...job, outcome: "probably-done" })).toThrow();
    expect(() =>
      parseFleetJob({
        ...job,
        targets: [
          { id: "xenia", state: "running", progress: { progress: Number.NaN, total: null } },
        ],
      }),
    ).toThrow();
  });

  it("requires a configuration revision for enabled fleet controls", () => {
    expect(parseFleetCatalog({ enabled: false, revision: null, actions: [] }).enabled).toBe(false);
    expect(() => parseFleetCatalog({ enabled: true, revision: null, actions: [] })).toThrow();
    expect(
      parseFleetCatalog({
        enabled: true,
        revision: "revision",
        actions: [{ action: "deploy", targets: ["xenia"] }],
      }).actions[0]?.targets,
    ).toEqual(["xenia"]);
  });

  it("generates distinct UUIDs without relying on secure-context randomUUID", () => {
    const first = serviceRequestId();
    expect(first).toMatch(/^[a-f0-9]{8}-[a-f0-9]{4}-4[a-f0-9]{3}-[89ab][a-f0-9]{3}-[a-f0-9]{12}$/);
    expect(serviceRequestId()).not.toBe(first);
  });
});
