import { describe, expect, it } from "vitest";
import {
  parseFleetCatalog,
  parseFleetJob,
  parseSetup,
  serviceRequestId,
  setupChain,
  setupHeadline,
} from "./services.svelte";

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

describe("agent setup", () => {
  const step = (component: string, state: string, message = `${component} ${state}`) => ({
    component,
    state,
    message,
    remediation: state === "present" ? null : `fix ${component}`,
  });
  const agent = (name: string, steps: unknown[], expected = true) => ({
    agent: name,
    expected,
    connected: false,
    steps,
  });
  const setup = (steps: unknown[], name = "codex") =>
    parseSetup({ build: "abcdef1234", agents: [agent(name, steps)] }).agents[0];
  const installed = [
    step("mcp", "present"),
    step("plugin", "present"),
    step("skill", "present"),
    step("hooks", "present"),
    step("hooks", "present"),
  ];

  it("shows a deployed plugin whose hooks still need approval", () => {
    const pending = setup([
      ...installed,
      step("hook_trust", "missing", "No trust record"),
      step("hook_trust", "missing", "No trust record"),
    ]);
    if (!pending) throw new Error("missing agent");
    expect(setupHeadline(pending)).toEqual({ state: "degraded", label: "Hooks need approval" });
    const chain = setupChain(pending);
    expect(chain.map((part) => [part.label, part.state])).toEqual([
      ["Connection", "ok"],
      ["Plugin", "ok"],
      ["Skill", "ok"],
      ["Hooks", "ok"],
      ["Hook approval", "attention"],
    ]);
    expect(chain.at(-1)?.notes).toEqual([
      { message: "No trust record", remediation: "fix hook_trust" },
    ]);
  });

  it("reports the earliest broken part and omits parts an agent lacks", () => {
    const stale = setup(
      [step("mcp", "present"), step("plugin", "outdated"), step("skill", "missing")],
      "claude-code",
    );
    if (!stale) throw new Error("missing agent");
    expect(setupHeadline(stale).label).toBe("Plugin missing or out of date");
    expect(setupChain(stale).map((part) => part.label)).toEqual(["Connection", "Plugin", "Skill"]);
  });

  it("does not present an approval it cannot verify as a failure", () => {
    const approved = setup([...installed, step("hook_trust", "unverified", "Record exists")]);
    if (!approved) throw new Error("missing agent");
    expect(setupHeadline(approved)).toEqual({
      state: "healthy",
      label: "Ready · hook approval recorded",
    });
    expect(setupChain(approved).at(-1)?.state).toBe("unverified");
    const absent = parseSetup({ build: "b", agents: [agent("codex", [], false)] }).agents[0];
    if (!absent) throw new Error("missing agent");
    expect(setupHeadline(absent).label).toBe("Not set up on this machine");
  });

  it("rejects unknown agents, components and states", () => {
    const body = (value: unknown) => ({ build: "b", agents: [value] });
    expect(() => parseSetup(body(agent("shell", [])))).toThrow();
    expect(() => parseSetup(body(agent("codex", [step("backdoor", "present")])))).toThrow();
    expect(() => parseSetup(body(agent("codex", [step("plugin", "probably")])))).toThrow();
  });
});
