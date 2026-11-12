<script lang="ts">
import Server from "@lucide/svelte/icons/server";
import { createQuery } from "@tanstack/svelte-query";
import {
  problems,
  serviceAccessOptions,
  serviceOptions,
  servicesHref,
} from "../lib/services.svelte";
import MachineSetup from "./MachineSetup.svelte";
import ServiceTiles from "./ServiceTiles.svelte";

// What a machine's own dashboard reports: its agents' setup and its services.
let { machineId }: { machineId: string } = $props();
const access = createQuery(() => serviceAccessOptions);
const enabled = $derived(access.data !== undefined);
const fleet = createQuery(() => serviceOptions(enabled));
const machine = $derived(fleet.data?.find((machine) => machine.id === machineId));
const issues = $derived(problems(machine ? [machine] : []));
</script>

{#if machine?.reachable}<MachineSetup {machineId} {enabled} />{/if}
<section>
  <h2 class="h2"><Server size={15} aria-hidden="true" />services</h2>
  {#if fleet.isPending && enabled}<p class="meta" role="status">Checking services…</p>{/if}
  {#if fleet.error}<p class="meta" role="alert">{fleet.error.message}</p>{/if}
  {#if fleet.data && !machine}<p class="meta">This machine is not in the dashboard's services configuration, so its services and agent setup are not reported.</p>{/if}
  {#if machine && !machine.reachable}<p><span class="warn">Could not be reached.</span> {issues[0]?.fix}</p>{/if}
  {#if machine?.reachable}
    <ServiceTiles machineId={machine.id} services={machine.services} />
    {#each issues.filter((issue) => issue.service) as issue (issue.service?.id)}
      <p class="meta" style:margin-top="8px"><strong style:font-weight="600">{issue.service?.name}:</strong> {issue.fix}{issue.actions.length ? " Open it to do that." : ""}</p>
    {/each}
    {#each machine.warnings as warning (warning)}<p class="meta" style:margin-top="8px">{warning}</p>{/each}
  {/if}
</section>
