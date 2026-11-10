<script lang="ts">
import Laptop from "@lucide/svelte/icons/laptop";
import { createQuery } from "@tanstack/svelte-query";
import { agents as agentInfo } from "../lib/agents";
import { groupChats, whenLabel } from "../lib/browse";
import { catalogOptions } from "../lib/queries";
import { agentHref, machineHref } from "../lib/route.svelte";
import AgentMark from "./AgentMark.svelte";
import QueryError from "./QueryError.svelte";
import Skeleton from "./Skeleton.svelte";

// The agents or machines that have chats, one tile each.
let { field }: { field: "agent" | "machine" } = $props();
const catalogue = createQuery(() => catalogOptions);
const chats = $derived(catalogue.data?.chats ?? []);
const groups = $derived(
  groupChats(chats, (chat) => (field === "agent" ? chat.agent : chat.machine)),
);
const now = Date.now();
</script>

<div>
  <h1 class="h1">{field === "agent" ? "Agents" : "Machines"}</h1>
  <p class="meta">{chats.length.toLocaleString()} chats {field === "agent" ? "from" : "on"} {groups.length} {field}{groups.length === 1 ? "" : "s"}</p>
</div>
<QueryError title="Couldn’t load chats" query={catalogue} />
{#if catalogue.isPending}<Skeleton rows={4} />{/if}
<div class="grid-tiles">
  {#each groups as group (group.key)}
    {@const agent = group.chats[0]?.agent}
    <a class="tile" href={field === "agent" ? agentHref(group.key) : machineHref(group.key)}>
      <span class="tile-h">
        {#if field === "agent" && agent}<AgentMark {agent} size={24} /><b>{agentInfo[agent].name}</b>{:else}<Laptop size={26} color="var(--blue)" aria-hidden="true" /><b>{group.key}</b>{/if}
      </span>
      <span class="big"><b>{group.chats.length.toLocaleString()}</b><span>chats</span></span>
      <span class="bar"><i style:width="{Math.max(2, Math.round((group.chats.length / Math.max(1, chats.length)) * 100))}%"></i></span>
      {#if field === "machine"}<span class="tile-m">{#each group.agents.slice(0, 4) as [name] (name)}<AgentMark agent={name} size={14} />{/each}</span>{/if}
      <span class="tile-f"><span>{field === "agent" ? group.machines.map(([machine]) => machine).join(", ") : `${group.projects.filter(([name]) => name).length} projects`}</span><span>{whenLabel(group.latestMs, now)}</span></span>
    </a>
  {/each}
</div>
