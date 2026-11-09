<script lang="ts">
import Laptop from "@lucide/svelte/icons/laptop";
import Search from "@lucide/svelte/icons/search";
import { createQuery } from "@tanstack/svelte-query";
import { agents as agentInfo } from "../lib/agents";
import type { Catalog } from "../lib/api";
import { navigate } from "../lib/navigation";
import { pageOf } from "../lib/pagination";
import { catalogOptions } from "../lib/queries";
import { reading } from "../lib/reading.svelte";
import { chatHref, recordsHref } from "../lib/route.svelte";
import type { ChatSummary } from "../lib/types";
import AgentMark from "./AgentMark.svelte";
import HomeServices from "./HomeServices.svelte";
import LoadError from "./LoadError.svelte";
import MemoryCategories from "./MemoryCategories.svelte";

let {
  section = "home",
  listPage = 1,
}: {
  section?: "home" | "recent" | "machines" | "agents" | "continue" | "status";
  listPage?: number;
} = $props();
const pageSize = 50;
const catalogue = createQuery(() => catalogOptions);
let loadError = $state<Error | null>(null);
$effect(() => {
  if (catalogue.error) loadError = catalogue.error;
  else if (!catalogue.isFetching) loadError = null;
});
let snapshot = $state<Catalog | null>(null);
$effect(() => {
  if (catalogue.data && !snapshot) snapshot = catalogue.data;
});
const data = $derived(section === "status" ? catalogue.data : (snapshot ?? catalogue.data));
const changed = $derived(
  snapshot &&
    catalogue.data &&
    JSON.stringify(snapshot.chats) !== JSON.stringify(catalogue.data.chats),
);
const machines = $derived([...new Set(data?.chats.map((chat) => chat.machine) ?? [])]);
const providers = $derived([...new Set(data?.chats.map((chat) => chat.agent) ?? [])]);
let query = $state("");
const chats = $derived(
  section === "continue"
    ? reading.positions.flatMap(
        (position) =>
          data?.chats.find((chat) => chat.id === position.id && chat.machine === position.node) ??
          [],
      )
    : section === "home"
      ? (data?.chats ?? []).slice(0, 8)
      : (data?.chats ?? []),
);
const paged = $derived(section === "recent" ? pageOf(chats, listPage, pageSize) : null);
const visible = $derived(paged?.items ?? chats);
let shownPage: number | undefined;
$effect(() => {
  if (shownPage !== undefined && shownPage !== listPage) window.scrollTo(0, 0);
  shownPage = listPage;
});
function href(chat: ChatSummary): string {
  const position = reading.positions.find(
    (position) => position.id === chat.id && position.node === chat.machine,
  );
  return chatHref(chat.id, chat.machine, position?.message ?? "", position?.shared ?? false);
}
function day(chat: ChatSummary): string {
  return chat.updatedMs
    ? new Date(chat.updatedMs).toLocaleDateString(undefined, { month: "short", day: "numeric" })
    : chat.updatedAt;
}
function time(chat: ChatSummary): string {
  return chat.updatedMs
    ? new Date(chat.updatedMs).toLocaleTimeString(undefined, {
        hour: "2-digit",
        minute: "2-digit",
        hour12: false,
      })
    : "";
}
function search(event: SubmitEvent): void {
  event.preventDefault();
  navigate(`#/search?${new URLSearchParams({ q: query })}`);
}
</script>

<main class="page-column home-page">
  <form class="search-field" onsubmit={search} role="search">
    <Search size={18} aria-hidden="true" />
    <input aria-label="Search your agent chats" type="search" placeholder="Search conversations and code" bind:value={query} />
    <button type="submit" class="search-submit">Search</button>
  </form>
  {#if section !== "status"}<button type="button" class="home-refresh" disabled={catalogue.isFetching} onclick={() => catalogue.refetch()}>{catalogue.isFetching ? "Checking…" : "Check for updates"}</button>{/if}
  {#if catalogue.isPending && !loadError}<p role="status">Loading chats…</p>{/if}
  {#if loadError}<LoadError title="Couldn’t load chats" message={loadError.message} retry={() => catalogue.refetch()} busy={catalogue.isFetching}/>{/if}
  {#if section === "status"}
    <section><h1>Index status</h1><p class="text-muted">Mash reads indexed history. Indexing runs separately.</p>
      <p>Local node: {data?.localNode ?? "Loading…"}</p>
      {#each data?.warnings ?? [] as warning (warning)}<p role="status">{warning}</p>{/each}
      <button type="button" class="plain-button" onclick={() => catalogue.refetch()}>Refresh status</button>
    </section>
  {:else if section === "machines" || section === "agents"}
    <section><h1>{section === "machines" ? "Machines" : "Agents"}</h1>
      <ul class="browse-list">
        {#if section === "machines"}
          {#each machines as machine (machine)}<li><a href={"#/search?" + new URLSearchParams({ machine })}><Laptop size={16} />{machine}</a></li>{/each}
        {:else}
          {#each providers as agent (agent)}<li><a href={"#/search?" + new URLSearchParams({ agent })}><AgentMark {agent} />{agentInfo[agent].name}</a></li>{/each}
        {/if}
      </ul>
    </section>
  {:else}
    {#each data?.warnings ?? [] as warning (warning)}<p role="status" class="text-sm text-muted">{warning}</p>{/each}
    <section aria-labelledby="recent">
      {#if changed}<button type="button" class="update-notice" onclick={() => { if (catalogue.data) snapshot = catalogue.data; }}>New or updated chats available. Show updates</button>{/if}
      <div class="section-heading"><h2 id="recent">{section === "continue" ? "Continue reading" : "Recent conversations"}</h2>{#if section === "home"}<a href="#/recent">View all</a>{/if}</div>
      {#if data && !chats.length}<p>{section === "continue" ? "Open a conversation to keep your reading position in this browser session." : "No indexed chats yet. Run Mash indexing, then refresh."}</p>{/if}
      <ul class="chat-list">
        {#each visible as chat, index (JSON.stringify([chat.machine, chat.id]))}
          {#if index === 0 || day(visible[index - 1] ?? chat) !== day(chat)}<li class="day-label">{day(chat)}</li>{/if}
          <li><a class="chat-row" href={href(chat)}>
            <span class="chat-time">{time(chat)}</span>
            <span class="chat-summary"><span class="chat-title">{chat.title}</span><span class="chat-metadata"><AgentMark agent={chat.agent} size={13} />{agentInfo[chat.agent].name}<span aria-hidden="true">·</span>{chat.project.split('/').filter(Boolean).at(-1) || 'No project'}<span aria-hidden="true">·</span>{chat.machine}</span></span>
          </a></li>
        {/each}
      </ul>
      {#if paged && paged.pageCount > 1}
        <nav class="pager" aria-label="Recent conversations pages">
          {#if paged.page > 1}<a rel="prev" href={`#/recent?page=${paged.page - 1}`}>Previous</a>{:else}<span aria-hidden="true"></span>{/if}
          <span aria-live="polite">{paged.first.toLocaleString()}–{paged.last.toLocaleString()} of {paged.total.toLocaleString()}</span>
          {#if paged.page < paged.pageCount}<a rel="next" href={`#/recent?page=${paged.page + 1}`}>Next</a>{:else}<span aria-hidden="true"></span>{/if}
        </nav>
      {/if}
    </section>
    {#if section === "home"}
        <div class="home-browse">
          <HomeServices {machines}/>
          <section aria-labelledby="home-agents">
            <div class="section-heading"><h2 id="home-agents"><a href="#/agents">Agents</a></h2></div>
            <ul class="home-browse-links">
              {#each providers as agent (agent)}<li><a href={"#/search?" + new URLSearchParams({ agent })}><AgentMark {agent} /><span>{agentInfo[agent].name}</span></a></li>{/each}
            </ul>
          </section>
        </div>
      <section aria-labelledby="home-records"><div class="section-heading"><h2 id="home-records"><a href={recordsHref()}>Memory</a></h2></div><MemoryCategories /></section>
    {/if}
  {/if}
</main>
