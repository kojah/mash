import { describe, expect, it } from "vitest";
import * as wire from "./services.svelte";
import {
  type Machine,
  machineState,
  problems,
  serviceRequestId,
  setupChain,
  setupFix,
  setupHeadline,
} from "./services.svelte";
import { trimmed } from "./trimmed";

const parseFleetCatalog = (body: unknown) => wire.parseFleetCatalog(trimmed(body));
const parseFleetJob = (body: unknown) => wire.parseFleetJob(trimmed(body));
const parseSetup = (body: unknown) => wire.parseSetup(trimmed(body));

describe("fleet answers", () => {
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
    expect(parseFleetJob(job).targets[0]?.detail).toBeNull();
    const found = { ...job.targets[0], detail: "Build 0123456789ab installed; local search ready" };
    expect(parseFleetJob({ ...job, targets: [found] }).targets[0]?.detail).toBe(found.detail);
    expect(parseFleetJob({ ...job, outcome: "unknown" }).outcome).toBe("unknown");
  });

  it("requires a configuration revision for enabled fleet controls", () => {
    expect(
      parseFleetCatalog({ enabled: false, revision: null, actions: [], services: [] }).enabled,
    ).toBe(false);
    expect(() =>
      parseFleetCatalog({ enabled: true, revision: null, actions: [], services: [] }),
    ).toThrow();
    expect(
      parseFleetCatalog({
        enabled: true,
        revision: "revision",
        actions: [{ action: "deploy", targets: ["shared", "xenia"] }],
        services: [{ id: "shared", name: "Shared server", machine: "xenia" }],
      }),
    ).toEqual({
      enabled: true,
      revision: "revision",
      actions: [{ action: "deploy", targets: ["shared", "xenia"] }],
      services: [{ id: "shared", name: "Shared server", machine: "xenia" }],
    });
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

  it("answers many failing checks with one cause and one fix", () => {
    const stale = setup(
      [
        step("mcp", "present"),
        step("plugin", "outdated", "version differs"),
        step("skill", "missing", "skill file missing"),
        step("skill", "invalid", "stamp differs"),
        step("hooks", "missing", "SessionStart missing"),
        step("hooks", "missing", "Stop missing"),
      ],
      "claude-code",
    );
    if (!stale) throw new Error("missing agent");
    const fix = setupFix(stale, "Claude Code");
    expect(fix).toMatchObject({ deploy: true, command: "mash init --yes --no-index", checks: 5 });
    expect(fix?.cause).toContain("does not match the Mash installed");

    const pending = setup([...installed, step("hook_trust", "missing", "No trust record")]);
    if (!pending) throw new Error("missing agent");
    expect(setupFix(pending, "Codex")).toMatchObject({ deploy: false, command: null, checks: 1 });
    const absent = setup([step("mcp", "missing", "not registered")]);
    if (!absent) throw new Error("missing agent");
    expect(setupFix(absent, "Codex")).toMatchObject({ deploy: false, checks: 1 });
    const ready = setup(installed);
    if (!ready) throw new Error("missing agent");
    expect(setupFix(ready, "Codex")).toBeNull();
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
});

describe("machine health", () => {
  const service = (kind: Machine["services"][number]["kind"], state: "healthy" | "degraded") => ({
    id: `${kind}-${state}`,
    name: kind,
    kind,
    state,
    summary: "",
    checked_at: 1,
    pid: null,
    actions: [],
    control_note: "",
    diagnostics: [],
    compatibility: null,
    control_revision: null,
    logs_available: false,
  });
  const machine = (services: Machine["services"]): Machine => ({
    id: "xenia",
    platform: "linux",
    checked_at: 1,
    reachable: true,
    services,
    warnings: [],
  });

  it("does not count outdated agent sessions or an unsupervised idle watcher against the machine", () => {
    expect(machineState(machine([service("sqlite", "healthy"), service("mcp", "degraded")]))).toBe(
      "healthy",
    );
    expect(machineState(machine([service("server", "degraded"), service("mcp", "degraded")]))).toBe(
      "degraded",
    );
    // Nothing supervises a watcher here, so one not running is a choice.
    const idle = { ...service("watcher", "healthy"), state: "stopped" as const };
    const unwatched = { ...idle, id: "watcher-leadership" };
    expect(machineState(machine([service("sqlite", "healthy"), unwatched]))).toBe("healthy");
    expect(machineState(machine([{ ...idle, id: "managed-watcher" }]))).toBe("stopped");
  });

  it("lists each fault with a way to put it right", () => {
    const stopped = {
      ...service("watcher", "healthy"),
      id: "managed-watcher",
      state: "stopped" as const,
      actions: ["start" as const],
    };
    const running = { ...service("server", "degraded"), actions: ["stop" as const] };
    const away = { ...machine([]), id: "macbook", reachable: false };
    const found = problems([
      machine([service("sqlite", "healthy"), service("mcp", "degraded"), stopped, running]),
      away,
    ]);
    expect(
      found.map((problem) => [problem.machine.id, problem.service?.id, problem.actions]),
    ).toEqual([
      ["xenia", "managed-watcher", ["start"]],
      // Stopping a degraded service does not repair it, so it is not offered.
      ["xenia", "server-degraded", []],
      ["macbook", undefined, []],
    ]);
    expect(found[0]?.fix).toBe("Start it to bring it back.");
    expect(found[2]?.fix).toContain("check again");
  });
});
