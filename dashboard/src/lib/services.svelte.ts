import { queryOptions } from "@tanstack/svelte-query";
import { stableRead } from "./queries";

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

function object(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw new Error("Invalid services response. Update the dashboard and retry.");
  return value as Record<string, unknown>;
}
function text(value: unknown): string {
  if (typeof value !== "string") throw new Error("Invalid service text field.");
  return value;
}
function integer(value: unknown): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0)
    throw new Error("Invalid service number.");
  return value;
}
function optionalInteger(value: unknown): number | null {
  return value === null ? null : integer(value);
}
function boolean(value: unknown): boolean {
  if (typeof value !== "boolean") throw new Error("Invalid service flag.");
  return value;
}
function list(value: unknown): unknown[] {
  if (!Array.isArray(value)) throw new Error("Invalid services list.");
  return value;
}
function choice<const T extends readonly string[]>(value: unknown, choices: T): T[number] {
  const result = text(value);
  if (!choices.includes(result)) throw new Error("Unsupported services response value.");
  return result as T[number];
}
const states = ["healthy", "degraded", "stopped", "failed", "unknown"] as const;
const actions = ["start", "stop", "restart"] as const;

function service(value: unknown): Service {
  const row = object(value);
  const compat = row.compatibility === null ? null : object(row.compatibility);
  return {
    id: text(row.id),
    name: text(row.name),
    kind: choice(row.kind, ["qdrant", "watcher", "mcp", "server", "dashboard", "sqlite"]),
    state: choice(row.state, states),
    summary: text(row.summary),
    checked_at: integer(row.checked_at),
    pid: optionalInteger(row.pid),
    actions: list(row.actions).map((action) => choice(action, actions)),
    control_note: text(row.control_note),
    diagnostics: list(row.diagnostics).map((value) => {
      const item = object(value);
      return { label: text(item.label), value: text(item.value) };
    }),
    logs_available: boolean(row.logs_available),
    control_revision: row.control_revision === null ? null : text(row.control_revision),
    compatibility:
      compat === null
        ? null
        : {
            database_schema: optionalInteger(compat.database_schema),
            supported_schema: optionalInteger(compat.supported_schema),
            running_build: text(compat.running_build),
            observer_build: text(compat.observer_build),
            state: choice(compat.state, states),
            blocked: text(compat.blocked),
            next_step: text(compat.next_step),
          },
  };
}
export function parseMachines(value: unknown): Machine[] {
  return list(object(value).machines).map((value) => {
    const row = object(value);
    return {
      id: text(row.id),
      platform: text(row.platform),
      checked_at: integer(row.checked_at),
      reachable: boolean(row.reachable),
      services: list(row.services).map(service),
      warnings: list(row.warnings).map(text),
    };
  });
}
function operation(value: unknown): Operation {
  const row = object(value),
    request = object(row.request);
  return {
    request: {
      machine: text(request.machine),
      service: text(request.service),
      action: choice(request.action, actions),
      request_id: text(request.request_id),
      control_revision: text(request.control_revision),
    },
    requested_at: integer(row.requested_at),
    completed_at: optionalInteger(row.completed_at),
    outcome: choice(row.outcome, ["pending", "succeeded", "failed", "unknown"]),
    message: text(row.message),
  };
}

let credential = "";
export const serviceSession = $state({ epoch: 0, unlocked: false });
export function unlockServices(token: string): void {
  credential = token;
  serviceSession.unlocked = !!token;
  serviceSession.epoch++;
}
async function request(path: string, signal?: AbortSignal, body?: unknown): Promise<unknown> {
  const response = await fetch(path, {
    method: body === undefined ? "GET" : "POST",
    cache: "no-store",
    headers: {
      ...(credential ? { Authorization: `Bearer ${credential}` } : {}),
      ...(body === undefined ? {} : { "Content-Type": "application/json" }),
    },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    signal: AbortSignal.any([...(signal ? [signal] : []), AbortSignal.timeout(90_000)]),
  });
  if (!response.ok) {
    let message =
      response.status === 401
        ? "Access token required or not accepted."
        : "Service request failed. Check status before repeating an action.";
    try {
      message = text(object(object(await response.json()).error).message);
    } catch {
      /* Preserve useful fallback for non-JSON failures. */
    }
    throw new Error(message);
  }
  return response.json();
}
export const serviceAccessOptions = queryOptions({
  ...stableRead,
  queryKey: ["service-access"],
  queryFn: async ({ signal }) => {
    const row = object(await request("/api/services/access", signal));
    return {
      authenticationRequired: boolean(row.authentication_required),
      localNode: text(row.local_node),
    };
  },
});
export function serviceOptions(enabled: boolean) {
  return queryOptions({
    ...stableRead,
    queryKey: ["services", serviceSession.epoch],
    enabled,
    queryFn: async ({ signal }) => parseMachines(await request("/api/services", signal)),
  });
}
export function servicesHref(machine = "", service = ""): string {
  return `#/services?${new URLSearchParams({ ...(machine ? { machine } : {}), ...(service ? { service } : {}) })}`;
}
export function machineState(machine: Machine): ServiceState {
  if (!machine.reachable || machine.services.length === 0) return "unknown";
  for (const state of ["failed", "unknown", "degraded", "stopped"] as const)
    if (machine.services.some((s) => s.state === state)) return state;
  return "healthy";
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
  return text(
    object(await request(`/api/services/log?${new URLSearchParams({ machine, service })}`, signal))
      .text,
  );
}
export async function serviceHistory(machine: string, signal: AbortSignal): Promise<Operation[]> {
  return list(
    await request(`/api/services/operations?${new URLSearchParams({ machine })}`, signal),
  ).map(operation);
}
export async function serviceAction(
  machine: string,
  service: string,
  action: ServiceAction,
  controlRevision: string,
): Promise<Operation> {
  return operation(
    await request("/api/services/action", undefined, {
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

const fleetActions = [
  "status",
  "preflight",
  "deploy",
  "sync",
  "local-views",
  "replay-views",
  "watch-start",
  "watch-stop",
  "watch-status",
  "watch-logs",
] as const;
export type FleetAction = (typeof fleetActions)[number];
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
export function parseFleetCatalog(value: unknown) {
  const row = object(value);
  const enabled = boolean(row.enabled);
  const revision = row.revision === null ? null : text(row.revision);
  if (enabled && !revision) throw new Error("Fleet configuration revision is missing.");
  return {
    enabled,
    revision,
    actions: list(row.actions).map((value) => {
      const action = object(value);
      return {
        action: choice(action.action, fleetActions),
        targets: list(action.targets).map(text),
      };
    }),
  };
}
export function parseFleetJob(value: unknown) {
  const row = object(value);
  const submitted = object(row.request);
  const request: FleetRequest = {
    id: text(submitted.id),
    action: choice(submitted.action, fleetActions),
    targets: list(submitted.targets).map(text),
    revision: text(submitted.revision),
  };
  return {
    request,
    requested_at: integer(row.requested_at),
    completed_at: optionalInteger(row.completed_at),
    outcome: choice(row.outcome, ["running", "succeeded", "partial", "failed", "unknown"]),
    targets: list(row.targets).map((value) => {
      const target = object(value);
      const progress = target.progress === null ? null : object(target.progress);
      return {
        id: text(target.id),
        state: choice(target.state, [
          "waiting",
          "running",
          "succeeded",
          "failed",
          "skipped",
          "unknown",
        ]),
        progress:
          progress === null
            ? null
            : {
                progress: finiteProgress(progress.progress),
                total: progress.total === null ? null : finiteProgress(progress.total),
              },
      };
    }),
  };
}
function finiteProgress(value: unknown): number {
  if (typeof value !== "number" || !Number.isFinite(value) || value < 0)
    throw new Error("Invalid fleet progress.");
  return value;
}
export async function fleetCatalog(signal: AbortSignal) {
  return parseFleetCatalog(await request("/api/services/fleet", signal));
}
export async function fleetHistory(signal: AbortSignal) {
  return list(await request("/api/services/fleet/jobs", signal)).map(parseFleetJob);
}
export async function submitFleetJob(submitted: FleetRequest) {
  return parseFleetJob(await request("/api/services/fleet/jobs", undefined, submitted));
}
