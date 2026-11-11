<script lang="ts">
import Check from "@lucide/svelte/icons/check";
import Clock from "@lucide/svelte/icons/clock";
import Database from "@lucide/svelte/icons/database";
import Eye from "@lucide/svelte/icons/eye";
import Info from "@lucide/svelte/icons/info";
import LayoutDashboard from "@lucide/svelte/icons/layout-dashboard";
import Plug from "@lucide/svelte/icons/plug";
import RefreshCw from "@lucide/svelte/icons/refresh-cw";
import ScrollText from "@lucide/svelte/icons/scroll-text";
import Search from "@lucide/svelte/icons/search";
import Server from "@lucide/svelte/icons/server";
import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
import { createQuery, useQueryClient } from "@tanstack/svelte-query";
import { stableRead } from "../lib/queries";
import {
  checkedTime,
  incidental,
  type Machine,
  machineState,
  type Operation,
  problems,
  type Service,
  type ServiceAction,
  type ServiceState,
  serviceAccessOptions,
  serviceAction,
  serviceHistory,
  serviceLog,
  serviceOptions,
  servicesHref,
} from "../lib/services.svelte";
import LoadError from "./LoadError.svelte";
import MachineSetup from "./MachineSetup.svelte";
import QueryError from "./QueryError.svelte";
import ServiceStatus from "./ServiceStatus.svelte";
import Skeleton from "./Skeleton.svelte";

let { params }: { params: string } = $props();
const queryClient = useQueryClient();
const selection = $derived(new URLSearchParams(params));
const machineId = $derived(selection.get("machine") ?? "");
const serviceId = $derived(selection.get("service") ?? "");
const access = createQuery(() => serviceAccessOptions);
const enabled = $derived(access.data !== undefined);
const fleet = createQuery(() => serviceOptions(enabled));
// Without a machine in the address, the page shows the local one.
const selectedId = $derived(machineId || access.data?.localNode || "");
const machine = $derived(fleet.data?.find((machine) => machine.id === selectedId));
const columns = [
  { kind: "sqlite", label: "index" },
  { kind: "qdrant", label: "search" },
  { kind: "server", label: "API" },
  { kind: "watcher", label: "watcher" },
  { kind: "mcp", label: "MCP" },
  { kind: "dashboard", label: "dash" },
] as const;
const kindIcon = {
  sqlite: Database,
  qdrant: Search,
  server: Server,
  watcher: Eye,
  mcp: Plug,
  dashboard: LayoutDashboard,
} as const;
const severity: readonly ServiceState[] = ["failed", "unknown", "degraded", "stopped", "healthy"];
/** The state a matrix cell shows: the worst among that machine's services of a
 * kind, or a plain note when the only thing to say is not a fault. */
function cell(
  services: readonly Service[],
  kind: Service["kind"],
): ServiceState | "reconnect" | "off" | null {
  const ofKind = services.filter((service) => service.kind === kind);
  const states = ofKind.filter((service) => !incidental(service)).map((service) => service.state);
  const worst = severity.find((state) => states.includes(state)) ?? null;
  if ((worst === null || worst === "healthy") && ofKind.some(incidental))
    return kind === "mcp" ? "reconnect" : "off";
  return worst;
}
const healthy = $derived(
  (fleet.data ?? []).filter((machine) => machineState(machine) === "healthy").length,
);
const service = $derived(machine?.services.find((service) => service.id === serviceId));
const history = createQuery(() => ({
  ...stableRead,
  queryKey: ["service-history", selectedId],
  enabled: enabled && !!selectedId,
  queryFn: ({ signal }) => serviceHistory(selectedId, signal),
}));
let showLog = $state(false);
const log = createQuery(() => ({
  ...stableRead,
  queryKey: ["service-log", selectedId, serviceId],
  enabled: enabled && showLog && !!serviceId,
  queryFn: ({ signal }) => serviceLog(selectedId, serviceId, signal),
}));
interface Target {
  machine: Machine;
  service: Service;
  action: ServiceAction;
}
let confirm = $state<Target | null>(null);
// Faults are shown with the machine they belong to, not across the fleet.
// The list shows every machine; choosing one in the table narrows it to that one.
const shown = $derived(
  (fleet.data ?? []).filter((candidate) => !machineId || candidate.id === machineId),
);
const issue = $derived(
  problems(machine ? [machine] : []).find((found) => found.service?.id === serviceId),
);
let pending = $state(false);
let actionError = $state("");
let result = $state<Operation | null>(null);
$effect(() => {
  void params;
  showLog = false;
  confirm = null;
  actionError = "";
});
async function refresh(): Promise<void> {
  const refreshed = await fleet.refetch();
  if (!refreshed.error) actionError = "";
  await queryClient.invalidateQueries({ queryKey: ["service-setup"] });
  if (selectedId) await history.refetch();
  if (showLog && serviceId) await log.refetch();
}
async function act(): Promise<void> {
  const revision = confirm?.service.control_revision;
  if (!confirm || !revision || pending) return;
  const action = confirm.action,
    target = confirm.machine.id,
    id = confirm.service.id;
  confirm = null;
  pending = true;
  actionError = "";
  result = null;
  try {
    result = await serviceAction(target, id, action, revision);
    await queryClient.invalidateQueries({ queryKey: ["services"] });
    await queryClient.invalidateQueries({ queryKey: ["service-history"] });
  } catch (error) {
    actionError =
      error instanceof Error
        ? error.message
        : "Operation outcome is unknown. Check status and recent activity.";
  } finally {
    pending = false;
  }
}
const actionLabel: Record<ServiceAction, string> = {
  start: "Start",
  stop: "Stop",
  restart: "Restart",
};
function consequences({ service, action }: Target): string {
  if (action === "start") return "";
  return service.kind === "watcher"
    ? "New chats may stop appearing until the watcher is running again. Existing indexed history is retained."
    : service.kind === "qdrant"
      ? "Search that uses this database may be unavailable while it is stopped or restarting."
      : "Clients using this service may lose access while it is stopped or restarting.";
}
const blocked = $derived(pending || !!fleet.error || !!actionError);
</script>

{#snippet confirmation(target: Target)}
  <div class="confirm" role="group" aria-label="Confirm service action" style:margin-top="10px">
    <span>{actionLabel[target.action]} {target.service.name} on {target.machine.id}? {consequences(target)}</span>
    <button class="btn p" type="button" onclick={act}>Confirm {target.action}</button>
    <button class="btn" type="button" onclick={() => (confirm = null)}>Cancel</button>
  </div>
{/snippet}

{#if serviceId}
  <a class="back" href={servicesHref(selectedId)}>&lt; services</a>
  <div class="hd">
    <div><h1 class="h1">{service?.name ?? serviceId}</h1><p class="meta">{selectedId}{service ? ` / ${service.kind}` : ""}</p></div>
    <span class="acts"><button class="btn" type="button" disabled={!enabled || fleet.isFetching || pending} onclick={refresh}><RefreshCw size={15} aria-hidden="true" />{fleet.isFetching ? "Checking…" : "Check again"}</button></span>
  </div>
{:else}
  <div class="hd">
    <h1 class="h1">Services</h1>
    <span class="acts">
      <span class="meta" style:font-size="13.5px" role="status">{fleet.data ? `${healthy} of ${fleet.data.length} machines healthy` : enabled ? "checking machines…" : ""}</span>
      <button class="btn" type="button" disabled={!enabled || fleet.isFetching || pending} onclick={refresh}><RefreshCw size={15} aria-hidden="true" />{fleet.isFetching ? "Checking…" : "Check again"}</button>
    </span>
  </div>
{/if}
<p class="meta">Statuses are from the last check. This page does not refresh on its own.</p>
<QueryError title="Couldn’t connect to services" query={access} />
{#if enabled && fleet.isPending}<Skeleton rows={3} />{/if}
{#if enabled && fleet.error}<LoadError title="Couldn’t check service status" message={fleet.error.message} retry={refresh} busy={fleet.isFetching}/><p class="meta warn">Statuses shown earlier have not been verified.</p>{/if}
{#if pending}<p class="meta" role="status">Service action in progress. Check recent activity if the connection is interrupted.</p>{/if}
{#if actionError}<p class="fail" role="alert">{actionError}</p>{/if}
{#if result}<p class="ok" role="status" style:font-size="13.5px"><strong>{actionLabel[result.request.action]} on {result.request.machine}: {result.outcome}.</strong> {result.message}</p>{/if}

{#if enabled && fleet.data && serviceId}
  {#if !machine}<div class="state-c"><p role="alert">This machine is not in the configured inventory.</p></div>{/if}
  {#if machine && !service}<div class="state-c"><p role="alert">This service was not reported by the latest check.</p></div>{/if}
  {#if machine && service}
    {@const compatibility = service.compatibility}
    <dl class="stat">
      <div><dt>status</dt><dd style:font-size="15px" style:display="flex" style:align-items="center" style:gap="9px"><i class="dot" class:warn={service.state !== "healthy"}></i>{machine.reachable ? service.summary : "Machine unreachable"}</dd><p class="meta">checked {checkedTime(service.checked_at)}{service.pid ? ` / PID ${service.pid}` : ""}</p></div>
      {#if compatibility}
        <div><dt>schema</dt><dd style:font-size="15px" style:display="flex" style:align-items="center" style:gap="9px"><i class="dot" class:warn={compatibility.state !== "healthy"}></i>{compatibility.state === "healthy" ? "Compatible" : "Needs attention"}</dd><p class="meta">database {compatibility.database_schema ?? "unknown"} / supported {compatibility.supported_schema ?? "unknown"}</p></div>
        <div><dt>build</dt><dd style:font-size="15px" style:overflow-wrap="anywhere">{compatibility.running_build.slice(0, 12) || "unknown"}</dd><p class="meta">dashboard {compatibility.observer_build.slice(0, 12)}</p></div>
      {/if}
    </dl>
    {#if compatibility && (compatibility.blocked || compatibility.next_step)}
      <section><h2 class="h2"><Info size={15} aria-hidden="true" />Schema compatibility</h2><p>{compatibility.blocked}</p><p class="meta" style:margin-top="6px">Next step</p><p>{compatibility.next_step}</p></section>
    {/if}
    <section>
      <h2 class="h2"><Server size={15} aria-hidden="true" />manage</h2>
      {#if issue}<p><span class="warn">Needs attention.</span> {issue.fix}</p>{/if}
      <p class="meta">{service.control_note}</p>
      <div class="chips" style:gap="8px" style:margin-top="8px">
        {#each service.actions as action (action)}<button class="btn" class:p={issue?.actions.includes(action)} type="button" disabled={blocked || !machine.reachable} onclick={() => (confirm = { machine, service, action })}>{actionLabel[action]}</button>{/each}
        {#if service.logs_available}<button class="btn" type="button" aria-expanded={showLog} onclick={() => (showLog = !showLog)}><ScrollText size={15} aria-hidden="true" />{showLog ? "Hide log" : "View log"}</button>{/if}
      </div>
      {#if confirm}{@render confirmation(confirm)}{/if}
      {#if showLog}
        {#if log.isPending}<p class="meta" role="status">Loading log…</p>{/if}
        {#if log.error}<p class="fail" role="alert">{log.error.message}</p>{/if}
        {#if log.data !== undefined}<div class="blk"><div class="sc"><pre class="service-log" style:margin="0" style:white-space="pre">{log.data || "No recent log entries."}</pre></div></div>{/if}
      {/if}
    </section>
    <section>
      <h2 class="h2"><Info size={15} aria-hidden="true" />Diagnostics</h2>
      <dl>{#each service.diagnostics as item (item.label)}<div class="kv"><dt>{item.label}</dt><dd style:overflow-wrap="anywhere">{item.value}</dd></div>{/each}</dl>
      {#if !service.diagnostics.length}<p class="meta">Nothing further was reported.</p>{/if}
    </section>
  {/if}
{:else if enabled && fleet.data}
  {#if machineId && !machine}<div class="state-c"><p role="alert">This machine is not in the configured inventory.</p></div>{/if}
  <div class="matrix">
    <div class="mx">
      <div class="mx-r head"><span style:padding-left="14px">machine</span>{#each columns as column (column.kind)}<span>{column.label}</span>{/each}</div>
      {#each fleet.data as item (item.id)}
        {@const state = machineState(item)}
        <a class="mx-r" class:on={item.id === machineId} href={servicesHref(item.id)} aria-current={item.id === machineId ? "true" : undefined} aria-label="{item.id}: {item.reachable ? (state === 'healthy' ? 'all services healthy' : 'needs attention') : 'unreachable'}">
          <span class="mx-n"><i class="dot" class:warn={state !== "healthy"}></i><span>{item.id}</span></span>
          {#each columns as column (column.kind)}
            {@const status = item.reachable ? cell(item.services, column.kind) : "unknown"}
            {#if status === null}<span class="cell none">–</span>
            {:else if status === "healthy"}<span class="cell ok"><Check size={16} aria-label="healthy" /></span>
            {:else if status === "reconnect" || status === "off"}<span class="cell none">{status}</span>
            {:else}<span class="cell warn"><TriangleAlert size={16} aria-hidden="true" />{status}</span>{/if}
          {/each}
        </a>
      {/each}
    </div>
  </div>
  {#if machineId && machine}<p class="meta"><a class="a" href={servicesHref()}>Show every machine</a></p>{/if}
  {#each shown as machine (machine.id)}
    {@const issues = problems([machine])}
    {#if issues.length}
      <section aria-label="Needs attention on {machine.id}">
        <h2 class="h2"><TriangleAlert size={15} aria-hidden="true" />needs attention on {machine.id}</h2>
        {#each issues as issue (`${issue.machine.id}/${issue.service?.id ?? ""}`)}
          {@const entry = issue.service}
          <div class="fixit">
            <div class="g">
              <p><strong style:font-weight="500">{entry ? entry.name : issue.machine.id}</strong> <span class="warn">{entry ? entry.summary : "Could not be reached"}</span></p>
              <p class="meta">{issue.fix}</p>
            </div>
            <span class="acts">
              {#if entry}
                {#each issue.actions as action (action)}<button class="btn p" type="button" disabled={blocked} onclick={() => (confirm = { machine: issue.machine, service: entry, action })}>{actionLabel[action]}</button>{/each}
                <a class="btn" href={servicesHref(issue.machine.id, entry.id)}>Details</a>
              {:else}
                <button class="btn" type="button" disabled={fleet.isFetching || pending} onclick={refresh}>Check again</button>
              {/if}
            </span>
            {#if entry && confirm?.service.id === entry.id && confirm.machine.id === issue.machine.id}{@render confirmation(confirm)}{/if}
          </div>
        {/each}
      </section>
    {/if}
    <section>
      <h2 class="h2"><Server size={15} aria-hidden="true" />services on {machine.id}</h2>
      <div class="svc-tiles">
        {#each machine.services as entry (entry.id)}
          {@const Icon = kindIcon[entry.kind]}
          <a class="svc-tile" href={servicesHref(machine.id, entry.id)}>
            <span class="svc-th"><Icon size={18} aria-hidden="true" /><strong>{entry.name}</strong></span>
            <ServiceStatus state={entry.state} label={entry.summary}/>
            <span class="mt">{entry.kind}</span>
          </a>
        {/each}
      </div>
      {#if !machine.services.length}<p class="meta">{machine.reachable ? "No services were reported." : "This machine's dashboard could not be reached."}</p>{/if}
      {#if machine.reachable}{#each machine.warnings as warning (warning)}<p class="meta" style:margin-top="8px">{warning}</p>{/each}{/if}
    </section>
    {#if machine.reachable}<MachineSetup machineId={machine.id} {enabled} />{/if}
  {/each}
{/if}
{#if enabled && selectedId}
  <section>
    <h2 class="h2"><Clock size={15} aria-hidden="true" />recent activity{serviceId ? "" : ` on ${selectedId}`}</h2>
    {#if history.error}<p class="fail" role="alert">{history.error.message}</p>{/if}
    {#if history.isPending}<p class="meta" role="status">Loading activity…</p>{/if}
    {#each history.data?.filter((item) => !serviceId || item.request.service === serviceId) ?? [] as item (item.request.request_id)}
      <div class="row"><span style:display="block"><strong style:font-weight="500">{actionLabel[item.request.action]} {item.request.service}: {item.outcome}</strong></span><span class="meta">{checkedTime(item.requested_at)}</span><p class="meta">{item.message}</p></div>
    {:else}
      {#if history.data}<p class="meta">No recorded service actions.</p>{/if}
    {/each}
  </section>
{/if}
