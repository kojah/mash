<script lang="ts">
import ArrowLeft from "@lucide/svelte/icons/arrow-left";
import RefreshCw from "@lucide/svelte/icons/refresh-cw";
import { createQuery, useQueryClient } from "@tanstack/svelte-query";
import { stableRead } from "../lib/queries";
import {
  checkedTime,
  machineState,
  type Operation,
  type ServiceAction,
  serviceAccessOptions,
  serviceAction,
  serviceHistory,
  serviceLog,
  serviceOptions,
  serviceSession,
  servicesHref,
  unlockServices,
} from "../lib/services.svelte";
import LoadError from "./LoadError.svelte";
import ServiceStatus from "./ServiceStatus.svelte";

let { params }: { params: string } = $props();
const queryClient = useQueryClient();
const selection = $derived(new URLSearchParams(params));
const machineId = $derived(selection.get("machine") ?? "");
const serviceId = $derived(selection.get("service") ?? "");
const access = createQuery(() => serviceAccessOptions);
const enabled = $derived(
  access.data !== undefined && (!access.data.authenticationRequired || serviceSession.unlocked),
);
const fleet = createQuery(() => serviceOptions(enabled));
const machine = $derived(fleet.data?.find((machine) => machine.id === machineId));
const service = $derived(machine?.services.find((service) => service.id === serviceId));
const history = createQuery(() => ({
  ...stableRead,
  queryKey: ["service-history", serviceSession.epoch, machineId],
  enabled: enabled && !!machineId,
  queryFn: ({ signal }) => serviceHistory(machineId, signal),
}));
let showLog = $state(false);
const log = createQuery(() => ({
  ...stableRead,
  queryKey: ["service-log", serviceSession.epoch, machineId, serviceId],
  enabled: enabled && showLog && !!serviceId,
  queryFn: ({ signal }) => serviceLog(machineId, serviceId, signal),
}));
let token = $state("");
let confirm = $state<ServiceAction | null>(null);
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
  if (machineId) await history.refetch();
  if (showLog && serviceId) await log.refetch();
}
async function act(): Promise<void> {
  if (!confirm || !machine || !service?.control_revision || pending) return;
  const action = confirm,
    target = machine.id,
    id = service.id;
  confirm = null;
  pending = true;
  actionError = "";
  result = null;
  try {
    result = await serviceAction(target, id, action, service.control_revision);
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
function unlock(event: SubmitEvent): void {
  event.preventDefault();
  unlockServices(token);
  token = "";
}
function lock(): void {
  unlockServices("");
  queryClient.removeQueries({ queryKey: ["services"] });
  queryClient.removeQueries({ queryKey: ["service-history"] });
  queryClient.removeQueries({ queryKey: ["service-log"] });
  result = null;
  actionError = "";
}
const actionLabel: Record<ServiceAction, string> = {
  start: "Start",
  stop: "Stop",
  restart: "Restart",
};
const consequences = $derived(
  service?.kind === "watcher"
    ? "New chats may stop appearing until the watcher is running again. Existing indexed history is retained."
    : service?.kind === "qdrant"
      ? "Search that uses this database may be unavailable while it is stopped or restarting."
      : "Clients using this service may lose access while it is stopped or restarting.",
);
</script>

<main class="page-column services-page">
  <a class="service-back" href={machineId ? servicesHref(serviceId ? machineId : "") : "#/"}><ArrowLeft size={14}/>{serviceId ? machineId : machineId ? "Services" : "Home"}</a>
  <div class="service-heading"><h1>{service?.name ?? machine?.id ?? "Services"}</h1><button class="plain-button" type="button" disabled={!enabled || fleet.isFetching || pending} onclick={refresh}><RefreshCw size={14}/>{fleet.isFetching ? "Checking…" : "Check for updates"}</button></div>
  <p class="text-muted">Statuses reflect the last check. This page does not refresh automatically.</p>
  {#if access.error}<LoadError title="Couldn’t connect to services" message={access.error.message} retry={() => access.refetch()} busy={access.isFetching}/>{/if}
  {#if access.data?.authenticationRequired}
    {#if !serviceSession.unlocked || fleet.error}<form class="service-unlock" onsubmit={unlock}><label for="service-token">Services access token</label><div><input id="service-token" type="password" autocomplete="off" bind:value={token} required/><button class="plain-button" type="submit">Unlock services</button></div><p class="text-muted">Kept only in this tab’s memory; never saved to browser storage.</p></form>
    {:else}<button type="button" class="service-lock" onclick={lock}>Lock services</button>{/if}
  {/if}
  {#if enabled && fleet.isPending}<p role="status">Checking machines…</p>{/if}
  {#if enabled && fleet.error}<LoadError title="Couldn’t check service status" message={fleet.error.message} retry={refresh} busy={fleet.isFetching}/><p class="service-warning">Previously displayed statuses have not been verified.</p>{/if}
  {#if pending}<p role="status">Service action in progress. You can check recent activity if the connection is interrupted.</p>{/if}
  {#if actionError}<p role="alert">{actionError}</p>{/if}
  {#if result}<p role="status"><strong>{actionLabel[result.request.action]} on {result.request.machine}: {result.outcome}.</strong> {result.message}</p>{/if}
  {#if enabled && service && machine}
    <ServiceStatus state={machine.reachable ? service.state : "unknown"} label={machine.reachable ? service.summary : "Machine unreachable · current status unknown"}/>
    <p class="service-checked">Checked {checkedTime(service.checked_at)}{service.pid ? ` · PID ${service.pid}` : ""}</p>
    {#if service.compatibility}
      {@const compatibility = service.compatibility}
      <section><h2>Schema compatibility</h2><ServiceStatus state={compatibility.state}/><dl class="service-diagnostics"><div><dt>Database schema</dt><dd>{compatibility.database_schema ?? "Unknown"}</dd></div><div><dt>Supported schema</dt><dd>{compatibility.supported_schema ?? "Unknown"}</dd></div><div><dt>Running build</dt><dd>{compatibility.running_build}</dd></div><div><dt>Dashboard build</dt><dd>{compatibility.observer_build}</dd></div></dl><p>{compatibility.blocked}</p><h3>Next step</h3><p>{compatibility.next_step}</p></section>
    {/if}
    <section><h2>Manage</h2><p class="text-muted">{service.control_note}</p><div class="service-actions">
      {#each service.actions as action (action)}<button class="plain-button" type="button" disabled={pending || !machine.reachable || !!fleet.error || !!actionError} onclick={() => confirm = action}>{actionLabel[action]}</button>{/each}
      {#if service.logs_available}<button class="plain-button" type="button" onclick={() => showLog = !showLog}>{showLog ? "Hide log" : "View log"}</button>{/if}
    </div>
    {#if confirm}<div class="service-confirm" role="group" aria-label="Confirm service action"><h3>{actionLabel[confirm]} {service.name} on {machine.id}?</h3><p>{consequences}</p><div class="service-actions"><button class="plain-button" type="button" onclick={act}>Confirm {confirm}</button><button class="plain-button" type="button" onclick={() => confirm = null}>Cancel</button></div></div>{/if}
    </section>
    <section><h2>Diagnostics</h2><dl class="service-diagnostics">{#each service.diagnostics as item (item.label)}<div><dt>{item.label}</dt><dd>{item.value}</dd></div>{/each}</dl></section>
    {#if showLog}<section><h2>Service log</h2>{#if log.isPending}<p role="status">Loading log…</p>{/if}{#if log.error}<p role="alert">{log.error.message}</p>{/if}{#if log.data !== undefined}<pre class="service-log">{log.data || "No recent log entries."}</pre>{/if}</section>{/if}
  {:else if enabled && fleet.data}
    {#if machineId && !machine}<p role="alert">This machine is not in the configured inventory.</p>{/if}
    {#if serviceId && machine}<p role="alert">This service was not reported by the latest check.</p>{/if}
    {#each fleet.data.filter(item => !machineId || item.id === machineId) as item (item.id)}
      <section class="service-machine"><div class="section-heading"><h2><a href={servicesHref(item.id)}>{item.id}</a></h2><span class="text-muted">{item.platform}</span></div><div class="service-machine-status"><ServiceStatus state={machineState(item)}/><span class="service-checked">Checked {checkedTime(item.checked_at)}</span></div>
      {#each item.warnings as warning (warning)}<p class="service-warning">{warning}</p>{/each}
      <ul class="service-list">{#each item.services as entry (entry.id)}<li><a href={servicesHref(item.id, entry.id)}><span><strong>{entry.name}</strong><small>{entry.kind}</small></span><ServiceStatus state={entry.state} label={entry.summary}/></a></li>{/each}</ul>
    </section>
    {/each}
  {/if}
  {#if enabled && machineId}<section><h2>Recent activity</h2>{#if history.error}<p role="alert">{history.error.message}</p>{/if}{#if history.isPending}<p role="status">Loading activity…</p>{/if}<ul class="service-activity">{#each history.data?.filter(item => !serviceId || item.request.service === serviceId) ?? [] as item (item.request.request_id)}<li><strong>{actionLabel[item.request.action]} · {item.outcome}</strong><span>{checkedTime(item.requested_at)} · {item.request.service}</span><p>{item.message}</p></li>{:else}{#if history.data}<li>No recorded service actions.</li>{/if}{/each}</ul></section>{/if}
</main>
