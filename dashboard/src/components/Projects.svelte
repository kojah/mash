<script module lang="ts">
import type { ProjectView } from "../lib/browse";

// Kept for the life of the page, so returning from a project keeps the view.
const view: ProjectView = $state({ find: "", agent: "", machine: "", sort: "chats" });
</script>

<script lang="ts">
import Bot from "@lucide/svelte/icons/bot";
import Folder from "@lucide/svelte/icons/folder";
import Laptop from "@lucide/svelte/icons/laptop";
import { createQuery } from "@tanstack/svelte-query";
import { agents as agentInfo } from "../lib/agents";
import { arrangeProjects, groupChats, projectLabel, whenLabel } from "../lib/browse";
import { catalogOptions } from "../lib/queries";
import { projectHref } from "../lib/route.svelte";
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
const shown = $derived(arrangeProjects(projects, view));
const largest = $derived(projects[0]?.chats.length ?? 1);
const agentChoices = $derived([...new Set(chats.map((chat) => chat.agent))].sort());
const machineChoices = $derived([...new Set(chats.map((chat) => chat.machine))].sort());
const narrowed = $derived(!!(view.find.trim() || view.agent || view.machine));
const now = Date.now();
</script>

<div><h1 class="h1">Projects</h1><p class="meta" role="status">{narrowed ? `${shown.length.toLocaleString()} of ${projects.length.toLocaleString()} projects` : `${projects.length.toLocaleString()} projects across ${chats.length.toLocaleString()} chats`}</p></div>
<QueryError title="Couldn’t load chats" query={catalogue} />
{#if catalogue.isPending}<Skeleton rows={5} />{/if}
{#if projects.length}
  <div class="pctl">
    <input class="field" type="search" aria-label="Find a project" placeholder="Find a project" autocomplete="off" bind:value={view.find} />
    <select aria-label="Sort projects" bind:value={view.sort}>
      <option value="chats">Most chats</option>
      <option value="recent">Most recent</option>
      <option value="name">Name</option>
    </select>
    <select aria-label="Agent" bind:value={view.agent}>
      <option value="">Any agent</option>
      {#each agentChoices as agent (agent)}<option value={agent}>{agentInfo[agent].name}</option>{/each}
    </select>
    <select aria-label="Machine" bind:value={view.machine}>
      <option value="">Any machine</option>
      {#each machineChoices as machine (machine)}<option value={machine}>{machine}</option>{/each}
    </select>
    {#if narrowed}<button class="a" type="button" onclick={() => { view.find = ""; view.agent = ""; view.machine = ""; }}>Show all</button>{/if}
  </div>
{/if}
{#if projects.length && !shown.length}<div class="state-c"><p>No project matches. Change the name, agent or machine above.</p></div>{/if}
<div class="grid-tiles">
  {#each shown as project (project.key)}
    <a class="tile" href={projectHref(project.key)} title={project.key}>
      <span class="tile-h"><Folder size={24} color="var(--warn)" aria-hidden="true" /><b style:min-width="0" style:overflow="hidden" style:text-overflow="ellipsis" style:white-space="nowrap">{projectLabel(project.key)}</b></span>
      <span class="big"><b>{project.chats.length.toLocaleString()}</b><span>chat{project.chats.length === 1 ? "" : "s"}</span></span>
      <span class="bar"><i style:width="{Math.max(2, Math.round((project.chats.length / largest) * 100))}%"></i></span>
      <span class="mts">
        {#each project.agents.slice(0, 4) as [agent] (agent)}<span class="mt"><Bot size={12} aria-hidden="true" />{agentInfo[agent].name}</span>{/each}
        {#each project.machines as [machine] (machine)}<span class="mt"><Laptop size={12} aria-hidden="true" />{machine}</span>{/each}
        <span class="mtx">{whenLabel(project.latestMs, now)}</span>
      </span>
    </a>
  {/each}
</div>
