<script lang="ts">
import Database from "@lucide/svelte/icons/database";
import Eye from "@lucide/svelte/icons/eye";
import LayoutDashboard from "@lucide/svelte/icons/layout-dashboard";
import Plug from "@lucide/svelte/icons/plug";
import Search from "@lucide/svelte/icons/search";
import Server from "@lucide/svelte/icons/server";
import { type Service, servicesHref } from "../lib/services.svelte";
import ServiceStatus from "./ServiceStatus.svelte";

// One machine's services as tiles, each opening that service's page.
let { machineId, services }: { machineId: string; services: readonly Service[] } = $props();
const kindIcon = {
  sqlite: Database,
  qdrant: Search,
  server: Server,
  watcher: Eye,
  mcp: Plug,
  dashboard: LayoutDashboard,
} as const;
</script>

<div class="svc-tiles">
  {#each services as entry (entry.id)}
    {@const Icon = kindIcon[entry.kind]}
    <a class="svc-tile" href={servicesHref(machineId, entry.id)}>
      <span class="svc-th"><Icon size={18} aria-hidden="true" /><strong>{entry.name}</strong></span>
      <ServiceStatus state={entry.state} label={entry.summary}/>
      <span class="mt">{entry.kind}</span>
    </a>
  {/each}
</div>
