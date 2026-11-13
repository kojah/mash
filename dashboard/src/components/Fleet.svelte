<script lang="ts">
import Download from "@lucide/svelte/icons/download";
import Eye from "@lucide/svelte/icons/eye";
import Laptop from "@lucide/svelte/icons/laptop";
import ListChecks from "@lucide/svelte/icons/list-checks";
import RefreshCw from "@lucide/svelte/icons/refresh-cw";
import ScrollText from "@lucide/svelte/icons/scroll-text";
import Server from "@lucide/svelte/icons/server";
import SlidersHorizontal from "@lucide/svelte/icons/sliders-horizontal";
import { createQuery, useQueryClient } from "@tanstack/svelte-query";
import { stableRead } from "../lib/queries";
import { fleetHref } from "../lib/route.svelte";
import {
  checkedTime,
  type FleetAction,
  type FleetRequest,
  fleetActionLabel,
  fleetCatalogOptions,
  fleetHistory,
  serviceAccessOptions,
  serviceRequestId,
  submitFleetJob,
} from "../lib/services.svelte";
import QueryError from "./QueryError.svelte";
import Skeleton from "./Skeleton.svelte";

// Fleet actions run through the controller's allowlist. The list page shows
// what this dashboard may start; an action's page picks its machines.
let { action }: { action: string } = $props();
const client = useQueryClient();
const access = createQuery(() => serviceAccessOptions);
const enabled = $derived(access.data !== undefined);
const catalog = createQuery(() => fleetCatalogOptions(enabled));
const history = createQuery(() => ({
  ...stableRead,
  queryKey: ["fleet-jobs"],
  enabled: enabled && catalog.data?.enabled === true,
  queryFn: ({ signal }) => fleetHistory(signal),
  refetchInterval: (query) =>
    query.state.data?.some((job) => job.outcome === "running") ? 2000 : false,
}));

const groups: readonly { name: string; actions: readonly FleetAction[] }[] = [
  { name: "check", actions: ["status", "preflight", "watch-status"] },
  { name: "update", actions: ["deploy"] },
  { name: "index", actions: ["sync", "local-views", "replay-views"] },
  { name: "watchers", actions: ["watch-start", "watch-stop", "watch-logs"] },
];
const blurb: Record<FleetAction, string> = {
  status: "Reads each machine's installed build and readiness. Changes nothing.",
  preflight: "Checks that a deployment could run. Changes nothing.",
  "watch-status": "Reads whether each machine's watcher is running. Changes nothing.",
  deploy: "Builds and installs Mash. Services can be interrupted while it runs.",
  sync: "Indexes new history and publishes it. Can take several minutes.",
  "local-views": "Rebuilds local search from stored history. Can take several minutes.",
  "replay-views": "Rebuilds shared search on the server. Can take several minutes.",
  "watch-start": "Installs the configured supervisor job and starts the watcher.",
  "watch-stop": "Stops the watcher. New chats stop appearing until it is started again.",
  "watch-logs": "Saves recent watcher logs privately on the controller.",
};
const icons = {
  status: Laptop,
  preflight: ListChecks,
  "watch-status": Eye,
  deploy: Download,
  sync: RefreshCw,
  "local-views": RefreshCw,
  "replay-views": RefreshCw,
  "watch-start": Eye,
  "watch-stop": Eye,
  "watch-logs": ScrollText,
} as const;
const readOnly: readonly FleetAction[] = ["status", "preflight", "watch-status"];

const allowed = $derived(catalog.data?.actions ?? []);
const current = $derived(allowed.find((item) => item.action === action));
// A target is a machine unless the inventory lists it as a shared server.
const services = $derived(catalog.data?.services ?? []);
const machineTargets = $derived(
  (current?.targets ?? []).filter((id) => !services.some((service) => service.id === id)),
);
const serviceTargets = $derived(
  services.filter((service) => current?.targets.includes(service.id)),
);
function targetName(id: string): string {
  const service = services.find((item) => item.id === id);
  return service ? `${service.name} (${service.machine})` : id;
}
function countLabel(ids: readonly string[]): string {
  const onlyMachines = ids.every((id) => machineTargets.includes(id));
  const noun = onlyMachines ? "machine" : "target";
  return `${ids.length} ${noun}${ids.length === 1 ? "" : "s"}`;
}
let selected = $state<string[]>([]);
let confirmation = $state<FleetRequest | null>(null);
let pending = $state(false);
let error = $state("");
$effect(() => {
  void action;
  selected = [];
  confirmation = null;
});
const running = $derived(history.data?.find((job) => job.outcome === "running"));
const jobs = $derived(
  (history.data ?? []).filter((job) => !action || job.request.action === action),
);

function toggle(machine: string): void {
  selected = selected.includes(machine)
    ? selected.filter((item) => item !== machine)
    : [...selected, machine];
  confirmation = null;
}
function prepare(): void {
  if (!catalog.data?.revision || !current || !selected.length) return;
  const request = {
    id: serviceRequestId(),
    action: current.action,
    targets: [...selected],
    revision: catalog.data.revision,
  };
  if (readOnly.includes(current.action)) void submit(request);
  else confirmation = request;
}
async function submit(request: FleetRequest): Promise<void> {
  if (pending) return;
  confirmation = null;
  pending = true;
  try {
    const job = await submitFleetJob(request);
    client.setQueryData<Awaited<ReturnType<typeof fleetHistory>>>(["fleet-jobs"], (previous) => [
      job,
      ...(previous ?? []).filter((item) => item.request.id !== job.request.id),
    ]);
    selected = [];
  } catch (cause) {
    error = `${cause instanceof Error ? cause.message : "Fleet request failed."} Request ${request.id}. Check the activity below and the affected machines before starting another operation.`;
  } finally {
    pending = false;
  }
}
const outcomeClass = {
  running: "run",
  succeeded: "ok",
  partial: "warn",
  failed: "fail",
  unknown: "warn",
} as const;
</script>

{#snippet activity()}
  <div class="hd"><h2 class="h2">{action ? "recent runs" : "activity"}</h2><button class="a meta" type="button" disabled={history.isFetching} onclick={() => history.refetch()} style:min-height="44px">check jobs</button></div>
  <QueryError title="Couldn’t check fleet jobs" query={history} />
  {#if history.data && !jobs.length}<p class="meta">No runs yet.</p>{/if}
  {#each jobs as job (job.request.id)}
    <article>
      <p style:display="flex" style:align-items="baseline" style:gap="10px"><i class="dot" class:run={job.outcome === "running"} class:bad={job.outcome === "failed"} class:warn={job.outcome === "partial" || job.outcome === "unknown"}></i><b style:flex="1" style:min-width="0">{fleetActionLabel[job.request.action]}</b><span class="meta">{checkedTime(job.requested_at)}</span></p>
      {#each job.targets as target (target.id)}
        <div class="elb" style:font-size="13.5px"><span style:flex="1">{targetName(target.id)}</span><span class={target.state === "succeeded" ? "ok" : target.state === "failed" ? "fail" : target.state === "running" ? "run" : "none"}>{target.state}{#if target.progress} {target.progress.progress}{target.progress.total === null ? "" : ` / ${target.progress.total}`}{/if}</span></div>
        {#if target.detail}<p class="meta" style:font-size="13px" style:margin="0 0 6px">{target.detail}</p>{/if}
      {/each}
      <p class={outcomeClass[job.outcome]} style:margin="4px 0 0 21px" style:font-size="13px">{job.outcome}{job.outcome === "unknown" ? ": completion could not be confirmed. Inspect the affected machines before starting another operation." : ""}</p>
    </article>
  {/each}
{/snippet}

{#if action}<a class="back" href="#/fleet">&lt; fleet manager</a>{/if}
<QueryError title="Couldn’t connect to services" query={access} />
<QueryError title="Couldn’t load fleet controls" query={catalog} />
{#if (access.isPending || catalog.isPending) && !access.error && !catalog.error}<h1 class="h1">Fleet manager</h1><Skeleton rows={3} />{/if}
{#if catalog.data && !catalog.data.enabled}
  <h1 class="h1">Fleet manager</h1>
  <div class="state-c">
    <p>Fleet control is not set up on this dashboard.</p>
    <p class="meta" style:margin-top="6px">It needs a fleet inventory that names this machine as the controller: start the dashboard with <code class="i">mash dash --fleet-config PATH</code>. Until then, deploys and index runs are started from the command line with <code class="i">tools/fleet.sh</code>.</p>
  </div>
{:else if catalog.data && !action}
  <h1 class="h1">Fleet manager</h1>
  <div class="cols">
    <div style:flex="999 1 400px" style:min-width="0">
      {#each groups as group (group.name)}
        {@const available = group.actions.filter((item) => allowed.some((entry) => entry.action === item))}
        {#if available.length}
          <section>
            <h2 class="h2" style:margin-top="12px">{group.name}</h2>
            <div class="grid-sm">
              {#each available as item (item)}
                {@const Icon = icons[item]}
                <a class="op" href={fleetHref(item)}><Icon size={20} aria-hidden="true" />{fleetActionLabel[item]}</a>
              {/each}
            </div>
          </section>
        {/if}
      {/each}
      {#if !allowed.length}<p class="meta">The inventory allows no actions from this dashboard.</p>{/if}
    </div>
    <div style:flex="1 1 320px" style:min-width="0" style:display="flex" style:flex-direction="column" style:gap="14px">{@render activity()}</div>
  </div>
{:else if catalog.data && action}
  {#if !current}
    <div class="state-c"><h1 class="h1">Not available</h1><p class="meta" style:margin-top="6px">This dashboard's inventory does not allow that action.</p></div>
  {:else}
    {@const Icon = icons[current.action]}
    <div style:display="flex" style:align-items="center" style:gap="12px"><Icon size={26} color="var(--teal)" aria-hidden="true" /><div><h1 class="h1">{fleetActionLabel[current.action]}</h1><p class="meta">{groups.find((group) => group.actions.includes(current.action))?.name}</p></div></div>
    <p>{blurb[current.action]}{readOnly.includes(current.action) ? "" : " It keeps running if you leave this page."}</p>
    <section>
      <div class="hd" style:align-items="center"><h2 class="h2"><Laptop size={15} aria-hidden="true" />machines</h2><button class="a" type="button" disabled={pending || !!error} onclick={() => { selected = [...current.targets]; confirmation = null; }} style:min-height="44px" style:font-size="14px">Select all</button></div>
      <div class="grid-sm">
        {#each machineTargets as machine (machine)}
          <button class="chk mtile" type="button" role="checkbox" aria-checked={selected.includes(machine)} disabled={pending || !!error} onclick={() => toggle(machine)}><i></i><span class="g">{machine}</span></button>
        {/each}
      </div>
      {#if !current.targets.length}<p class="meta">Nothing in the inventory is eligible for this action.</p>{/if}
    </section>
    {#if serviceTargets.length}
      <section>
        <div class="hd" style:align-items="center"><h2 class="h2"><Server size={15} aria-hidden="true" />services</h2></div>
        <div class="grid-sm">
          {#each serviceTargets as service (service.id)}
            <button class="chk mtile" type="button" role="checkbox" aria-label={service.name} aria-checked={selected.includes(service.id)} disabled={pending || !!error} onclick={() => toggle(service.id)}><i></i><span class="g">{service.name}</span><span class="meta">on {service.machine}</span></button>
          {/each}
        </div>
      </section>
    {/if}
    {#if running}<p class="meta" style:font-size="14px" role="status">{fleetActionLabel[running.request.action]} is running. Only one fleet operation can run at a time.</p>{/if}
    {#if confirmation}
      <div class="armed" role="group" aria-label="Confirm">
        <p>{fleetActionLabel[confirmation.action]} on {confirmation.targets.map(targetName).join(", ")}? {blurb[confirmation.action]}</p>
        <button class="btn p" type="button" disabled={pending} onclick={() => { if (confirmation) void submit(confirmation); }}>Yes, start it</button>
        <button class="btn" type="button" onclick={() => (confirmation = null)}>Cancel</button>
      </div>
    {:else}
      <div><button class="btn p" type="button" disabled={!selected.length || pending || !!error || !!running} onclick={prepare}><SlidersHorizontal size={15} aria-hidden="true" />{running ? "Unavailable while another job runs" : selected.length ? `${fleetActionLabel[current.action]} on ${countLabel(selected)}` : serviceTargets.length ? "Choose targets" : "Choose machines"}</button></div>
    {/if}
    {#if pending}<p class="meta" role="status">Recording the fleet operation…</p>{/if}
    {#if error}<p class="fail" role="alert">{error}</p><div><button class="btn" type="button" onclick={() => (error = "")}>I’ve checked the outcome</button></div>{/if}
    <section style:display="flex" style:flex-direction="column" style:gap="14px">{@render activity()}</section>
  {/if}
{/if}
