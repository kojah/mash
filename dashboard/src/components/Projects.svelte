<script module lang="ts">
import type { ProjectView } from "../lib/browse";

// Kept for the life of the page, so returning from a project keeps the view.
const view: ProjectView = $state({ find: "", agent: "", machine: "", sort: "chats" });
</script>

<script lang="ts">
import ArrowDownWideNarrow from "@lucide/svelte/icons/arrow-down-wide-narrow";
import Bot from "@lucide/svelte/icons/bot";
import Folder from "@lucide/svelte/icons/folder";
import Laptop from "@lucide/svelte/icons/laptop";
import { createQuery } from "@tanstack/svelte-query";
import { agents as agentInfo } from "../lib/agents";
import { arrangeProjects, groupChats, projectLabel, whenLabel } from "../lib/browse";
import { catalogOptions } from "../lib/queries";
import { projectHref } from "../lib/route.svelte";
import FilterMenu from "./FilterMenu.svelte";
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
const chosenAgent = $derived(agentChoices.find((agent) => agent === view.agent));
// Drawing every tile at once makes the page slow to open, so tiles are added
// as the reader nears the end of those drawn so far.
const STEP = 60;
let limit = $state(STEP);
let more = $state<HTMLElement>();
$effect(() => {
  if (!more) return;
  const watcher = new IntersectionObserver(
    (entries) => {
      if (entries.some((entry) => entry.isIntersecting)) limit += STEP;
    },
    { rootMargin: "600px" },
  );
  watcher.observe(more);
  return () => watcher.disconnect();
});
const sorts = [
  { value: "chats", label: "most chats" },
  { value: "recent", label: "most recent" },
  { value: "name", label: "by name" },
] as const;
const now = Date.now();
</script>

<div><h1 class="h1">Projects</h1><p class="meta" role="status">{narrowed ? `${shown.length.toLocaleString()} of ${projects.length.toLocaleString()} projects` : `${projects.length.toLocaleString()} projects across ${chats.length.toLocaleString()} chats`}</p></div>
<QueryError title="Couldn’t load chats" query={catalogue} />
{#if catalogue.isPending}<Skeleton rows={5} />{/if}
{#if projects.length}
  <div class="chips" role="group" aria-label="Project filters">
    <input class="field" type="search" aria-label="Find a project" placeholder="find a project" autocomplete="off" bind:value={view.find} style:flex="1 1 220px" style:max-width="360px" />
    <FilterMenu label={sorts.find((sort) => sort.value === view.sort)?.label ?? "sort"} options={sorts} selected={[view.sort]} choose={(value) => { const sort = sorts.find((sort) => sort.value === value); if (sort) view.sort = sort.value; }}>
      {#snippet icon()}<ArrowDownWideNarrow size={15} aria-hidden="true" />{/snippet}
    </FilterMenu>
    <FilterMenu label={chosenAgent ? agentInfo[chosenAgent].name : "any agent"} active={!!view.agent} options={[{ value: "", label: "any agent" }, ...agentChoices.map((agent) => ({ value: agent, label: agentInfo[agent].name }))]} selected={[view.agent]} choose={(value) => (view.agent = value)}>
      {#snippet icon()}<Bot size={15} aria-hidden="true" />{/snippet}
    </FilterMenu>
    <FilterMenu label={view.machine || "any machine"} active={!!view.machine} options={[{ value: "", label: "any machine" }, ...machineChoices.map((machine) => ({ value: machine, label: machine }))]} selected={[view.machine]} choose={(value) => (view.machine = value)}>
      {#snippet icon()}<Laptop size={15} aria-hidden="true" />{/snippet}
    </FilterMenu>
    {#if narrowed}<button class="a" type="button" style:min-height="30px" style:padding="0 6px" style:font-size="13.5px" onclick={() => { view.find = ""; view.agent = ""; view.machine = ""; }}>Show all</button>{/if}
  </div>
{/if}
{#if projects.length && !shown.length}<div class="state-c"><p>No project matches. Change the name, agent or machine above.</p></div>{/if}
<div class="grid-tiles">
  {#each shown.slice(0, limit) as project (project.key)}
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
{#if shown.length > limit}{#key limit}<div bind:this={more} aria-hidden="true" style:height="1px"></div>{/key}{/if}
