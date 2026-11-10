<script lang="ts">
import Server from "@lucide/svelte/icons/server";
import { createQuery } from "@tanstack/svelte-query";
import { serviceAccessOptions, serviceOptions, servicesHref } from "../lib/services.svelte";
import MachineSetup from "./MachineSetup.svelte";
import ServiceStatus from "./ServiceStatus.svelte";

// What a machine's own dashboard reports: its agents' setup and its services.
let { machineId }: { machineId: string } = $props();
const access = createQuery(() => serviceAccessOptions);
const enabled = $derived(access.data !== undefined);
const fleet = createQuery(() => serviceOptions(enabled));
const machine = $derived(fleet.data?.find((machine) => machine.id === machineId));
</script>

{#if machine?.reachable}<MachineSetup {machineId} {enabled} />{/if}
<section>
  <h2 class="h2"><Server size={15} aria-hidden="true" />services</h2>
  {#if fleet.isPending && enabled}<p class="meta" role="status">Checking services…</p>{/if}
  {#if fleet.error}<p class="meta" role="alert">{fleet.error.message}</p>{/if}
  {#if fleet.data && !machine}<p class="meta">This machine is not in the dashboard's services configuration, so its services and agent setup are not reported.</p>{/if}
  {#if machine && !machine.reachable}<p class="meta warn">This machine's dashboard could not be reached, so its services and agent setup are unknown.</p>{/if}
  {#if machine}
    {#each machine.warnings as warning (warning)}<p class="meta">{warning}</p>{/each}
    {#each machine.services as service (service.id)}
      <a class="li" href={servicesHref(machine.id, service.id)}><Server size={16} color="var(--teal)" aria-hidden="true" /><span class="g">{service.name}</span><ServiceStatus state={service.state} label={service.summary} /></a>
    {/each}
  {/if}
</section>
