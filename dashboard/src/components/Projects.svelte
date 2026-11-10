<script lang="ts">
import Folder from "@lucide/svelte/icons/folder";
import { createQuery } from "@tanstack/svelte-query";
import { groupChats, projectLabel, whenLabel } from "../lib/browse";
import { catalogOptions } from "../lib/queries";
import { projectHref } from "../lib/route.svelte";
import AgentMark from "./AgentMark.svelte";
import QueryError from "./QueryError.svelte";
import Skeleton from "./Skeleton.svelte";

const catalogue = createQuery(() => catalogOptions);
const chats = $derived(catalogue.data?.chats ?? []);
const projects = $derived(
  groupChats(
    chats.filter((chat) => chat.project),
    (chat) => chat.project,
  ),
);
const largest = $derived(projects[0]?.chats.length ?? 1);
const now = Date.now();
</script>

<div><h1 class="h1">Projects</h1><p class="meta">{projects.length.toLocaleString()} projects across {chats.length.toLocaleString()} chats</p></div>
<QueryError title="Couldn’t load chats" query={catalogue} />
{#if catalogue.isPending}<Skeleton rows={5} />{/if}
<div class="grid-tiles">
  {#each projects as project (project.key)}
    <a class="tile" href={projectHref(project.key)} title={project.key}>
      <span class="tile-h"><Folder size={24} color="var(--warn)" aria-hidden="true" /><b style:min-width="0" style:overflow="hidden" style:text-overflow="ellipsis" style:white-space="nowrap">{projectLabel(project.key)}</b></span>
      <span class="big"><b>{project.chats.length.toLocaleString()}</b><span>chat{project.chats.length === 1 ? "" : "s"}</span></span>
      <span class="bar"><i style:width="{Math.max(2, Math.round((project.chats.length / largest) * 100))}%"></i></span>
      <span class="tile-m">{#each project.agents.slice(0, 4) as [agent] (agent)}<AgentMark {agent} size={14} />{/each}</span>
      <span class="tile-f"><span>{project.machines.map(([machine]) => machine).join(", ")}</span><span>{whenLabel(project.latestMs, now)}</span></span>
    </a>
  {/each}
</div>
