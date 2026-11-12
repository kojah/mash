import { queryOptions } from "@tanstack/svelte-query";
import { stableRead } from "./queries";
import type { Answer, Changed, ReadPath, Sent, Wire, WritePath } from "./routes";
import { send } from "./transport";

export type ServiceState = "healthy" | "degraded" | "stopped" | "failed" | "unknown";
export type ServiceAction = "start" | "stop" | "restart";
export interface Service {
  id: string;
  name: string;
  kind: "qdrant" | "watcher" | "mcp" | "server" | "dashboard" | "sqlite";
  state: ServiceState;
  summary: string;
  checked_at: number;
  pid: number | null;
  actions: ServiceAction[];
  control_note: string;
  diagnostics: { label: string; value: string }[];
  logs_available: boolean;
  control_revision: string | null;
  compatibility: {
    database_schema: number | null;
    supported_schema: number | null;
    running_build: string;
    observer_build: string;
    state: ServiceState;
    blocked: string;
    next_step: string;
  } | null;
}
export interface Machine {
  id: string;
  platform: string;
  checked_at: number;
  reachable: boolean;
  services: Service[];
  warnings: string[];
}
export interface Operation {
  request: {
    machine: string;
    service: string;
    action: ServiceAction;
    request_id: string;
    control_revision: string;
  };
  requested_at: number;
  completed_at: number | null;
  outcome: "pending" | "succeeded" | "failed" | "unknown";
  message: string;
}

function service(row: Wire["Service"]): Service {
  return {
    ...row,
    pid: row.pid ?? null,
    control_revision: row.control_revision ?? null,
    compatibility: row.compatibility
      ? {
          ...row.compatibility,
          database_schema: row.compatibility.database_schema ?? null,
          supported_schema: row.compatibility.supported_schema ?? null,
        }
      : null,
  };
}
export function parseMachines(body: Answer<"/api/services">): Machine[] {
  return body.machines.map((row) => ({ ...row, services: row.services.map(service) }));
}
function operation(row: Wire["ServiceOperation"]): Operation {
  return { ...row, completed_at: row.completed_at ?? null };
}

async function call(
  path: string,
  signal: AbortSignal | undefined,
  body?: unknown,
): Promise<unknown> {
  const response = await send(path, {
    method: body === undefined ? "GET" : "POST",
    cache: "no-store",
    ...(body === undefined ? {} : { headers: { "Content-Type": "application/json" } }),
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    signal: AbortSignal.any([...(signal ? [signal] : []), AbortSignal.timeout(90_000)]),
  });
  if (!response.ok) {
    let message =
      response.status === 401
        ? "Services refused this request. Remove token_env from this dashboard's services configuration."
        : "Service request failed. Check status before repeating an action.";
    try {
      message = ((await response.json()) as Wire["Failure"]).error.message;
    } catch {
      /* Preserve useful fallback for non-JSON failures. */
    }
    throw new Error(message);
  }
  return response.json();
}
/** Reads a service route. The answer's type is the contract's for that route. */
export async function get<P extends ReadPath>(
  path: P,
  signal?: AbortSignal,
  params?: Record<string, string>,
): Promise<Answer<P>> {
  const query = params ? `?${new URLSearchParams(params)}` : "";
  return (await call(`${path}${query}`, signal)) as Answer<P>;
}
/** Sends a change to a service route, typed by the contract both ways. */
export async function post<P extends WritePath>(path: P, body: Sent<P>): Promise<Changed<P>> {
  return (await call(path, undefined, body)) as Changed<P>;
}
export type SetupComponent = Wire["Component"];
export type SetupState = Wire["SetupState"];
export interface SetupStep {
  component: SetupComponent;
  state: SetupState;
  message: string;
  remediation: string | null;
}
export interface AgentSetup {
  agent: "codex" | "claude-code";
  expected: boolean;
  connected: boolean;
  steps: SetupStep[];
}
/** One machine's agent integrations, as its own Mash binary inspected them. */
export interface Setup {
  build: string;
  agents: AgentSetup[];
}
export function parseSetup(row: Answer<"/api/services/installation">): Setup {
  return {
    build: row.build,
    agents: row.agents.map((agent) => ({
      ...agent,
      steps: agent.steps.map((step) => ({ ...step, remediation: step.remediation ?? null })),
    })),
  };
}

export type SetupPartState = "ok" | "attention" | "unverified";
export interface SetupPart {
  label: string;
  state: SetupPartState;
  /** What needs doing, shortest first, without repeats. */
  notes: { message: string; remediation: string | null }[];
}
const attention: readonly SetupState[] = ["missing", "disabled", "invalid", "outdated", "failed"];
const parts: readonly { component: SetupComponent; label: string }[] = [
  { component: "mcp", label: "Connection" },
  { component: "plugin", label: "Plugin" },
  { component: "skill", label: "Skill" },
  { component: "hooks", label: "Hooks" },
  { component: "hook_trust", label: "Hook approval" },
];
/**
 * An agent's integration in the order it is installed: each part depends on
 * the ones before it. A part the agent has no concept of is left out.
 */
export function setupChain(agent: AgentSetup): SetupPart[] {
  return parts.flatMap(({ component, label }) => {
    const steps = agent.steps.filter((step) => step.component === component);
    if (steps.length === 0) return [];
    const failing = steps.filter((step) => attention.includes(step.state));
    const state: SetupPartState =
      failing.length > 0
        ? "attention"
        : steps.some((step) => step.state === "unverified")
          ? "unverified"
          : "ok";
    const seen = new Set<string>();
    const notes = (state === "ok" ? [] : state === "attention" ? failing : steps)
      .filter((step) => state === "attention" || step.state === "unverified")
      .filter((step) => !seen.has(step.message) && seen.add(step.message))
      .map(({ message, remediation }) => ({ message, remediation }));
    return [{ label, state, notes }];
  });
}
const headlines: Record<string, string> = {
  Connection: "Not connected to Mash",
  Plugin: "Plugin missing or out of date",
  Skill: "Skill missing or out of date",
  Hooks: "Hooks missing or changed",
  "Hook approval": "Hooks need approval",
};
/** The first thing standing between this agent and a working integration. */
export function setupHeadline(agent: AgentSetup): { state: ServiceState; label: string } {
  if (!agent.expected) return { state: "unknown", label: "Not set up on this machine" };
  const chain = setupChain(agent);
  const blocked = chain.find((part) => part.state === "attention");
  if (blocked) return { state: "degraded", label: headlines[blocked.label] ?? blocked.label };
  return chain.some((part) => part.label === "Hook approval" && part.state === "unverified")
    ? { state: "healthy", label: "Ready · hook approval recorded" }
    : { state: "healthy", label: "Ready" };
}
/** The one thing to do about an agent's setup, however many checks failed. */
export interface SetupFix {
  /** What is wrong, in a sentence. */
  cause: string;
  /** What to do, in order. */
  steps: string[];
  /** A command to run on that machine, when one does the job. */
  command: string | null;
  /** Which step the command belongs to, counted from zero. */
  commandStep: number;
  /** Whether updating Mash on the machine is part of the fix. */
  deploy: boolean;
  /** How many separate checks this one fix answers. */
  checks: number;
}
/**
 * Failing checks share a cause far more often than not: a plugin that does not
 * match the installed Mash fails its version, its skill and every hook at
 * once. This names the earliest broken part and the one remedy for it.
 */
export function setupFix(agent: AgentSetup, name: string): SetupFix | null {
  if (!agent.expected) return null;
  const chain = setupChain(agent);
  const failing = chain.filter((part) => part.state === "attention");
  const first = failing[0];
  if (!first) return null;
  const checks = failing.reduce((total, part) => total + part.notes.length, 0);
  if (first.label === "Hook approval")
    return {
      cause: `${name} has not approved the Mash plugin's hooks, so they do not run.`,
      steps: [`Open /hooks in ${name}, trust the Mash hooks, then start a new session.`],
      command: null,
      commandStep: 0,
      deploy: false,
      checks,
    };
  if (first.label === "Connection")
    return {
      cause: `${name} does not have Mash registered on this machine.`,
      steps: ["Run this on the machine:", `Restart ${name}.`],
      command: "mash init --yes --no-index",
      commandStep: 0,
      deploy: false,
      checks,
    };
  return {
    cause: `The Mash plugin in ${name} does not match the Mash installed on this machine. They have to be the same release.`,
    steps: [
      "Update Mash on the machine.",
      "Reinstall the plugin by running this there:",
      `Restart ${name}.`,
    ],
    command: "mash init --yes --no-index",
    commandStep: 1,
    deploy: true,
    checks,
  };
}
export function setupOptions(machine: string, enabled: boolean) {
  return queryOptions({
    ...stableRead,
    queryKey: ["service-setup", machine],
    enabled: enabled && !!machine,
    queryFn: async ({ signal }) =>
      parseSetup(await get("/api/services/installation", signal, { machine })),
  });
}

export const serviceAccessOptions = queryOptions({
  ...stableRead,
  queryKey: ["service-access"],
  queryFn: async ({ signal }) => {
    const row = await get("/api/services/access", signal);
    return { localNode: row.local_node };
  },
});
export function serviceOptions(enabled: boolean) {
  return queryOptions({
    ...stableRead,
    queryKey: ["services"],
    enabled,
    queryFn: async ({ signal }) => parseMachines(await get("/api/services", signal)),
  });
}
export function servicesHref(machine = "", service = ""): string {
  return `#/services?${new URLSearchParams({ ...(machine ? { machine } : {}), ...(service ? { service } : {}) })}`;
}
/** A row worth showing that is not a fault of the machine: an agent's own Mash
 * session that predates the installed build (the agent reconnects it), or the
 * opt-in watcher not running where nothing supervises one. */
export function incidental(service: Service): boolean {
  return (
    (service.kind === "mcp" && service.state === "degraded") ||
    (service.id === "watcher-leadership" && service.state === "stopped")
  );
}
export function machineState(machine: Machine): ServiceState {
  if (!machine.reachable || machine.services.length === 0) return "unknown";
  const services = machine.services.filter((service) => !incidental(service));
  for (const state of ["failed", "unknown", "degraded", "stopped"] as const)
    if (services.some((s) => s.state === state)) return state;
  return "healthy";
}
/** Something on a machine that is not healthy, with what to do about it. */
export interface Problem {
  machine: Machine;
  /** Absent when the machine itself could not be reached. */
  service: Service | null;
  fix: string;
  /** The controls that could put it right; stopping never does. */
  actions: ServiceAction[];
}
function remedy(service: Service, actions: readonly ServiceAction[]): string {
  if (service.compatibility?.next_step) return service.compatibility.next_step;
  if (actions.includes("start")) return "Start it to bring it back.";
  if (actions.includes("restart")) return "Restart it, then check again.";
  if (service.kind === "mcp") return "Reconnect Mash in the agent that owns this session.";
  return service.control_note || "Open it for the details of the last check.";
}
/** Everything counted against these machines, in order: what a reader works
 * through to get a machine back to healthy. */
export function problems(machines: readonly Machine[]): Problem[] {
  return machines.flatMap((machine): Problem[] => {
    if (!machine.reachable)
      return [
        {
          machine,
          service: null,
          fix: "It may be asleep or off the network. Wake it, then check again; it cannot be managed from here until it answers.",
          actions: [],
        },
      ];
    return machine.services
      .filter((service) => service.state !== "healthy" && !incidental(service))
      .map((service) => {
        const actions = service.actions.filter((action) => action !== "stop");
        return { machine, service, fix: remedy(service, actions), actions };
      });
  });
}
export const stateLabel: Record<ServiceState, string> = {
  healthy: "Healthy",
  degraded: "Needs attention",
  stopped: "Stopped",
  failed: "Action needed",
  unknown: "Unknown",
};
export const checkedTime = (timestamp: number): string =>
  new Date(timestamp * 1000).toLocaleString();
export async function serviceLog(
  machine: string,
  service: string,
  signal: AbortSignal,
): Promise<string> {
  return (await get("/api/services/log", signal, { machine, service })).text;
}
export async function serviceHistory(machine: string, signal: AbortSignal): Promise<Operation[]> {
  return (await get("/api/services/operations", signal, { machine })).map(operation);
}
export async function serviceAction(
  machine: string,
  service: string,
  action: ServiceAction,
  controlRevision: string,
): Promise<Operation> {
  return operation(
    await post("/api/services/action", {
      machine,
      service,
      action,
      control_revision: controlRevision,
      request_id: serviceRequestId(),
    }),
  );
}

/** getRandomValues also works on private HTTP dashboards without a secure context. */
export function serviceRequestId(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  bytes[6] = ((bytes[6] ?? 0) & 15) | 64;
  bytes[8] = ((bytes[8] ?? 0) & 63) | 128;
  const hex = Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

export type FleetAction = Wire["FleetAction"];
export interface FleetRequest {
  id: string;
  action: FleetAction;
  targets: string[];
  revision: string;
}
export const fleetActionLabel: Record<FleetAction, string> = {
  status: "Check machines",
  preflight: "Check deployment readiness",
  deploy: "Deploy Mash",
  sync: "Index and publish",
  "local-views": "Rebuild local search",
  "replay-views": "Rebuild shared search",
  "watch-start": "Set up and start watchers",
  "watch-stop": "Stop watchers",
  "watch-status": "Check watchers",
  "watch-logs": "Collect watcher logs",
};
export function parseFleetCatalog(row: Answer<"/api/services/fleet">) {
  const revision = row.revision ?? null;
  if (row.enabled && !revision) throw new Error("Fleet configuration revision is missing.");
  // `services` are targets that are a shared server on a machine; every other target is a machine.
  return { enabled: row.enabled, revision, actions: row.actions, services: row.services };
}
export function parseFleetJob(row: Wire["Job"]) {
  return {
    request: row.request satisfies FleetRequest,
    requested_at: row.requested_at,
    completed_at: row.completed_at ?? null,
    outcome: row.outcome,
    targets: row.targets.map((target) => ({
      id: target.id,
      state: target.state,
      detail: target.detail ?? null,
      progress: target.progress
        ? { progress: target.progress.progress, total: target.progress.total ?? null }
        : null,
    })),
  };
}
export async function fleetCatalog(signal: AbortSignal) {
  return parseFleetCatalog(await get("/api/services/fleet", signal));
}
export async function fleetHistory(signal: AbortSignal) {
  return (await get("/api/services/fleet/jobs", signal)).map(parseFleetJob);
}
export async function submitFleetJob(submitted: FleetRequest) {
  return parseFleetJob(await post("/api/services/fleet/jobs", submitted));
}
