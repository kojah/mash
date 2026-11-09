<script lang="ts">
import Laptop from "@lucide/svelte/icons/laptop";
import { createQuery } from "@tanstack/svelte-query";
import {
  machineState,
  serviceAccessOptions,
  serviceOptions,
  servicesHref,
} from "../lib/services.svelte";
import ServiceStatus from "./ServiceStatus.svelte";

let { machines = [] }: { machines?: readonly string[] } = $props();

const access = createQuery(() => serviceAccessOptions);
const fleet = createQuery(() => serviceOptions(access.data !== undefined));
const names = $derived([
  ...new Set([...machines, ...(fleet.data?.map((machine) => machine.id) ?? [])]),
]);
</script>
<section aria-labelledby="home-machines"><div class="section-heading"><h2 id="home-machines"><a href="#/machines">Machines</a></h2><a href={servicesHref()}>Services</a></div>
<ul class="home-service-list">{#each names.slice(0, 6) as name (name)}{@const machine = fleet.data?.find(machine => machine.id === name)}<li><a href={"#/search?" + new URLSearchParams({machine:name})}><Laptop size={14} aria-hidden="true"/>{name}</a><a href={servicesHref(machine?.id ?? "")} aria-label={name + " service status"}><ServiceStatus state={machine && !fleet.error ? machineState(machine) : "unknown"}/></a></li>{/each}</ul>
{#if names.length > 6}<a class="service-warning" href={servicesHref()}>View all machines</a>{/if}
{#if fleet.error || access.error}<p class="service-warning"><a href={servicesHref()}>Service status unavailable</a></p>{/if}
</section>
