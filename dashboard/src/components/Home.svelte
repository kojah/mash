<script lang="ts">
import Bot from "@lucide/svelte/icons/bot";
import ChevronRight from "@lucide/svelte/icons/chevron-right";
import Clock from "@lucide/svelte/icons/clock";
import Laptop from "@lucide/svelte/icons/laptop";
import NotebookText from "@lucide/svelte/icons/notebook-text";
import Search from "@lucide/svelte/icons/search";
import { createQuery } from "@tanstack/svelte-query";
import { agents as agentInfo } from "../lib/agents";
import { catalogOptions } from "../lib/queries";
import type { Kind } from "../lib/records";
import { chatHref, recordsHref } from "../lib/route.svelte";
import type { SearchFilters } from "../lib/types";
import AgentMark from "./AgentMark.svelte";
import SearchPanel from "./SearchPanel.svelte";

const catalogue = createQuery(() => catalogOptions);
const data = $derived(catalogue.data);
const machines = $derived([...new Set(data?.chats.map((chat) => chat.machine) ?? [])]);
const agents = $derived([...new Set(data?.chats.map((chat) => chat.agent) ?? [])]);
let initial = $state<SearchFilters>({ agents: [], machines: [], projects: [], days: null });
let searching = $state(false);
const recordCategories: readonly { kind: Kind; label: string }[] = [
  { kind: "decision", label: "Decisions" },
  { kind: "preference", label: "Preferences" },
  { kind: "fact", label: "Facts" },
  { kind: "open_question", label: "Questions" },
];

function onKey(event: KeyboardEvent): void {
  if (event.key === "Escape") searching = false;
}
</script>

<svelte:window onkeydown={onKey} />

<main class="mx-auto flex max-w-[600px] flex-col gap-9 px-5 py-16">
  <nav class="flex gap-5 text-sm"><span aria-current="page" class="font-semibold">Chats</span><a href="#/records">Memory</a></nav>
  <div class="relative flex items-center">
    <Search size={20} strokeWidth={1.75} class="pointer-events-none absolute left-4 text-faint" />
    <label for="home-search" class="sr-only">Search your agent chats</label>
    <input
      id="home-search"
      type="search"
      placeholder="Search chats"
      readonly
      onfocus={() => (searching = true)}
      onclick={() => (searching = true)}
      class="min-h-[52px] w-full cursor-text rounded-xl border border-line bg-surface py-0 pr-[18px] pl-[46px] text-[17px] text-ink"
    />
  </div>

  <section aria-labelledby="home-records" class="flex flex-col gap-2">
    <h2 id="home-records" class="m-0 flex items-center gap-2 text-[13px] font-normal text-muted"><NotebookText size={15} strokeWidth={1.75} />Memory</h2>
    <ul class="m-0 flex list-none flex-col overflow-hidden rounded-xl bg-surface p-0">
      {#each recordCategories as category (category.kind)}
        <li><a class="row text-[15px]" href={recordsHref(new URLSearchParams({ kind: category.kind }))}><span class="flex-1">{category.label}</span><ChevronRight size={14} strokeWidth={2.25} class="shrink-0 text-chevron" /></a></li>
      {/each}
    </ul>
  </section>

  <p class="m-0 text-xs text-muted">Indexed history · refreshes every 30 seconds while visible. Indexing runs separately.</p>
  {#if catalogue.isPending}
    <section role="status" class="rounded-xl bg-surface p-4">
      <p class="m-0 text-sm text-muted">Loading chats…</p>
      <div aria-hidden="true" class="mt-4 flex flex-col gap-5">
        {#each [0, 1, 2, 3, 4] as row (row)}<div class="flex flex-col gap-2"><div class="h-4 w-4/5 rounded bg-fill"></div><div class="h-3 w-2/5 rounded bg-fill"></div></div>{/each}
      </div>
    </section>
  {/if}
  {#if catalogue.error}<p role="alert">{catalogue.error.message} <button type="button" onclick={() => catalogue.refetch()}>Retry</button></p>{/if}
  {#if data}
    {#each data.warnings as warning (warning)}<p role="status" class="text-sm text-muted">{warning}</p>{/each}
    {#if !data.chats.length}<p>No indexed chats yet. Run Mash indexing, then refresh.</p>{/if}
    <section aria-labelledby="recent" class="flex flex-col gap-2">
      <h2 id="recent" class="m-0 flex items-center gap-2 text-[13px] font-normal text-muted"><Clock size={15} strokeWidth={1.75} />Recent</h2>
      <ul class="m-0 flex list-none flex-col overflow-hidden rounded-xl bg-surface p-0">
        {#each data.chats.slice(0, 5) as chat (JSON.stringify([chat.machine, chat.id]))}
          <li>
            <a href={chatHref(chat.id, chat.machine)} title={`${chat.title}\n${chat.project}\n${chat.id}`} class="row py-2 text-[15px]">
              <AgentMark agent={chat.agent} />
              <span class="min-w-0 flex-1"><span class="line-clamp-2">{chat.title}</span><span class="block text-xs text-muted">{chat.project.split('/').filter(Boolean).at(-1) || 'No project'} · {chat.machine} · {chat.updatedAt}</span></span>
              <ChevronRight size={14} strokeWidth={2.25} class="shrink-0 text-chevron" />
            </a>
          </li>
        {/each}
      </ul>
    </section>

    <div class="grid items-start gap-x-5 gap-y-7 [grid-template-columns:repeat(auto-fit,minmax(240px,1fr))]">
      <section aria-labelledby="machines" class="flex flex-col gap-2">
        <h2 id="machines" class="m-0 flex items-center gap-2 text-[13px] font-normal text-muted"><Laptop size={15} strokeWidth={1.75} />Machines</h2>
        <ul class="m-0 flex list-none flex-col overflow-hidden rounded-xl bg-surface p-0">
          {#each machines as machine (machine)}
            <li>
              <button type="button" class="row w-full text-left text-[15px]" onclick={() => { initial = { agents: [], machines: [machine], projects: [], days: null }; searching = true; }}>
                <Laptop size={18} strokeWidth={1.75} class="shrink-0 text-ink-2" />
                <span class="flex-1">{machine}</span><span class="text-xs text-muted">{data.chats.filter(chat => chat.machine === machine).length}</span>
                <ChevronRight size={14} strokeWidth={2.25} class="shrink-0 text-chevron" />
              </button>
            </li>
          {/each}
        </ul>
      </section>

      <section aria-labelledby="agents" class="flex flex-col gap-2">
        <h2 id="agents" class="m-0 flex items-center gap-2 text-[13px] font-normal text-muted"><Bot size={15} strokeWidth={1.75} />Agents</h2>
        <ul class="m-0 flex list-none flex-col overflow-hidden rounded-xl bg-surface p-0">
          {#each agents as agent (agent)}
            <li>
              <button type="button" class="row w-full text-left text-[15px]" onclick={() => { initial = { agents: [agent], machines: [], projects: [], days: null }; searching = true; }}>
                <AgentMark agent={agent} />
                <span class="min-w-0 flex-1 truncate">{agentInfo[agent].name}</span><span class="text-xs text-muted">{data.chats.filter(chat => chat.agent === agent).length}</span>
                <ChevronRight size={14} strokeWidth={2.25} class="shrink-0 text-chevron" />
              </button>
            </li>
          {/each}
        </ul>
      </section>
    </div>
  {/if}
</main>

{#if searching}
  <button type="button" aria-label="Close search" class="fixed inset-0 z-10 cursor-default border-0 bg-black/30" onclick={() => (searching = false)}></button>
  <div role="dialog" aria-modal="true" aria-label="Search" class="fixed top-[12vh] left-1/2 z-20 flex max-h-[76vh] w-[min(720px,calc(100vw-32px))] -translate-x-1/2 flex-col rounded-[18px] bg-ground p-6 shadow-[0_24px_60px_rgba(0,0,0,0.2)]">
    <SearchPanel {initial} onClose={() => (searching = false)} />
  </div>
{/if}
