<script lang="ts">
import Bot from "@lucide/svelte/icons/bot";
import Server from "@lucide/svelte/icons/server";
import { createQuery } from "@tanstack/svelte-query";
import {
  problems,
  serviceAccessOptions,
  serviceOptions,
  servicesHref,
} from "../lib/services.svelte";
import MachineSetup from "./MachineSetup.svelte";
import ServiceRows from "./ServiceRows.svelte";

// What a machine's own dashboard reports: its agents' setup and its services.
let { machineId }: { machineId: string } = $props();
const access = createQuery(() => serviceAccessOptions);
const enabled = $derived(access.data !== undefined);
const fleet = createQuery(() => serviceOptions(enabled));
const machine = $derived(fleet.data?.find((machine) => machine.id === machineId));
const issues = $derived(problems(machine ? [machine] : []));
</script>

{#if machine?.reachable}
  <section>
    <h2 class="h2"><Bot size={15} aria-hidden="true" />agent setup</h2>
    <div class="rws setup solo">
      <div class="rw head" aria-hidden="true"><span>agent</span><span>parts</span><span>status</span></div>
      <MachineSetup {machineId} {enabled} showMachine={false} />
    </div>
    <p class="meta" style:margin-top="10px">Mash reads settings and files for this; it does not start the agents.</p>
  </section>
{/if}
<section>
  <h2 class="h2"><Server size={15} aria-hidden="true" />services</h2>
  {#if fleet.isPending && enabled}<p class="meta" role="status">Checking services…</p>{/if}
  {#if fleet.error}<p class="meta" role="alert">{fleet.error.message}</p>{/if}
  {#if fleet.data && !machine}<p class="meta">This machine is not in the dashboard's services configuration, so its services and agent setup are not reported.</p>{/if}
  {#if machine && !machine.reachable}<p><span class="warn">Could not be reached.</span> {issues[0]?.fix}</p>{/if}
  {#if machine?.reachable}
    <ServiceRows machines={[machine]} showMachine={false} />
    {#each issues.filter((issue) => issue.service) as issue (issue.service?.id)}
      <p class="meta" style:margin-top="8px"><strong style:font-weight="600">{issue.service?.name}:</strong> {issue.fix}{issue.actions.length ? " Open it to do that." : ""}</p>
    {/each}
    {#each machine.warnings as warning (warning)}<p class="meta" style:margin-top="8px">{warning}</p>{/each}
  {/if}
</section>
