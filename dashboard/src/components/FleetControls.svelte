<script lang="ts">
import { createQuery, useQueryClient } from "@tanstack/svelte-query";
import { stableRead } from "../lib/queries";
import {
  checkedTime,
  type FleetAction,
  type FleetRequest,
  fleetActionLabel,
  fleetCatalog,
  fleetHistory,
  serviceRequestId,
  submitFleetJob,
} from "../lib/services.svelte";
import LoadError from "./LoadError.svelte";

const { enabled }: { enabled: boolean } = $props();
const client = useQueryClient();
const catalog = createQuery(() => ({
  ...stableRead,
  queryKey: ["fleet-catalog"],
  enabled,
  queryFn: ({ signal }) => fleetCatalog(signal),
}));
const history = createQuery(() => ({
  ...stableRead,
  queryKey: ["fleet-jobs"],
  enabled: enabled && catalog.data?.enabled === true,
  queryFn: ({ signal }) => fleetHistory(signal),
  refetchInterval: (query) =>
    query.state.data?.some((job) => job.outcome === "running") ? 2000 : false,
}));
let action = $state<FleetAction>("status");
let selected = $state<string[]>([]);
let confirmation = $state<FleetRequest | null>(null);
let pending = $state(false);
let error = $state("");
const available = $derived(catalog.data?.actions.find((item) => item.action === action));
$effect(() => {
  const first = catalog.data?.actions[0];
  if (first && !available) {
    action = first.action;
    selected = [];
    confirmation = null;
  }
});

function prepare(): void {
  if (!catalog.data?.revision || !selected.length) return;
  confirmation = {
    id: serviceRequestId(),
    action,
    targets: [...selected],
    revision: catalog.data.revision,
  };
}
async function submit(): Promise<void> {
  if (!confirmation || pending) return;
  const request = confirmation;
  const key = ["fleet-jobs"];
  confirmation = null;
  pending = true;
  try {
    const job = await submitFleetJob(request);
    client.setQueryData<Awaited<ReturnType<typeof fleetHistory>>>(key, (previous) => [
      job,
      ...(previous ?? []).filter((item) => item.request.id !== job.request.id),
    ]);
    selected = [];
  } catch (cause) {
    error = `${cause instanceof Error ? cause.message : "Fleet request failed."} Request ${request.id}. Check job history and the affected machines before starting another operation.`;
  } finally {
    pending = false;
  }
}
async function refresh(): Promise<void> {
  await Promise.all([catalog.refetch(), history.refetch()]);
}
</script>

{#if enabled}
  {#if catalog.error}<LoadError title="Couldn’t load fleet controls" message={catalog.error.message} retry={() => catalog.refetch()} busy={catalog.isFetching}/>{/if}
  {#if catalog.data?.enabled}
    <section class="fleet-controls">
      <div class="section-heading"><h2>Fleet operations</h2><button class="plain-button" type="button" onclick={refresh} disabled={pending || history.isFetching}>Check jobs</button></div>
      <p class="text-muted">Choose an operation and machines. Running job progress updates here without reloading the page.</p>
      <label for="fleet-action">Operation</label>
      <select id="fleet-action" bind:value={action} disabled={pending || !!error} onchange={() => { selected = []; confirmation = null; }}>
        <option value="" disabled>Select an operation</option>
        {#each catalog.data.actions as item (item.action)}<option value={item.action}>{fleetActionLabel[item.action]}</option>{/each}
      </select>
      <fieldset disabled={pending || !!error}><legend>Machines</legend>
        <button class="plain-button" type="button" onclick={() => { selected = [...(available?.targets ?? [])]; confirmation = null; }}>Select all eligible machines</button>
        {#each available?.targets ?? [] as machine (machine)}<label><input type="checkbox" value={machine} bind:group={selected} onchange={() => confirmation = null}/>{machine}</label>{/each}
      </fieldset>
      <button class="plain-button" type="button" disabled={!selected.length || pending || !!error || !!catalog.error} onclick={prepare}>Review operation</button>
      {#if confirmation}<div class="service-confirm" role="group" aria-label="Confirm fleet operation">
        <h3>{fleetActionLabel[confirmation.action]}</h3><p>Machines: {confirmation.targets.join(", ")}</p>
        <p>Deployment can interrupt services. Indexing and search rebuilds can take several minutes. Watcher setup installs the configured supervisor job. This operation continues if you leave the page.</p>
        <button class="plain-button" type="button" onclick={submit} disabled={pending}>Confirm operation</button>
        <button class="plain-button" type="button" onclick={() => confirmation = null}>Cancel</button>
      </div>{/if}
      {#if pending}<p role="status">Recording fleet operation…</p>{/if}
      {#if error}<p role="alert">{error}</p><button class="plain-button" type="button" onclick={() => error = ""}>I’ve checked the outcome</button>{/if}
      {#if history.error}<LoadError title="Couldn’t check fleet jobs" message={history.error.message} retry={() => history.refetch()} busy={history.isFetching}/>{/if}
      <ul class="service-activity">{#each history.data ?? [] as job (job.request.id)}<li>
        <strong>{fleetActionLabel[job.request.action]} · {job.outcome}</strong><span>{checkedTime(job.requested_at)}</span>
        <ul>{#each job.targets as target (target.id)}<li>{target.id}: {target.state}{#if target.progress} · {target.progress.progress}{target.progress.total === null ? "" : ` / ${target.progress.total}`}{/if}</li>{/each}</ul>
        {#if job.outcome === "unknown"}<p>Completion could not be confirmed. Inspect the affected machines before starting another operation.</p>{/if}
        <small>Request {job.request.id}</small>
      </li>{:else}<li>No recorded fleet operations.</li>{/each}</ul>
    </section>
  {/if}
{/if}

<style>
  .fleet-controls { margin-block: 1.5rem; }
  select { display: block; width: 100%; margin-block: .5rem 1rem; padding: .6rem; border: 1px solid var(--border); border-radius: .4rem; background: transparent; font: inherit; }
  fieldset { display: flex; flex-wrap: wrap; gap: 1rem; margin-block: 1rem; }
  fieldset label { display: flex; align-items: center; gap: .5rem; }
  small { overflow-wrap: anywhere; }
</style>
