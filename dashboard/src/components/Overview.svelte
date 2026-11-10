<script lang="ts">
import BookOpen from "@lucide/svelte/icons/book-open";
import Bot from "@lucide/svelte/icons/bot";
import Clock from "@lucide/svelte/icons/clock";
import Laptop from "@lucide/svelte/icons/laptop";
import { createQuery } from "@tanstack/svelte-query";
import { agents as agentInfo } from "../lib/agents";
import { groupChats } from "../lib/browse";
import { catalogOptions, stableRead } from "../lib/queries";
import { recordPage } from "../lib/records";
import { agentHref, machineHref } from "../lib/route.svelte";
import AgentMark from "./AgentMark.svelte";
import ChatRow from "./ChatRow.svelte";
import MemoryRow from "./MemoryRow.svelte";
import QueryError from "./QueryError.svelte";
import Skeleton from "./Skeleton.svelte";

const catalogue = createQuery(() => catalogOptions);
const memories = createQuery(() => ({
  ...stableRead,
  queryKey: ["records", ""],
  queryFn: ({ signal }: { signal: AbortSignal }) => recordPage("", signal),
}));
// Lists change only when the reader asks: nothing refreshes on its own.
async function refresh(): Promise<void> {
  await Promise.all([catalogue.refetch(), memories.refetch()]);
}
const chats = $derived(catalogue.data?.chats ?? []);
const recent = $derived(chats.slice(0, 8));
const machines = $derived(groupChats(chats, (chat) => chat.machine));
const providers = $derived(groupChats(chats, (chat) => chat.agent));
</script>

<h1 class="sr-only">Overview</h1>
<QueryError title="Couldn’t load chats" query={catalogue} />
<div class="cols">
  <section class="c-main">
    <div class="hd">
      <h2 class="h2"><Clock size={15} aria-hidden="true" />recent chats</h2>
      <span class="acts">
        <button class="a meta" type="button" disabled={catalogue.isFetching} onclick={refresh} style:min-height="44px">{catalogue.isFetching ? "Checking…" : "Check for updates"}</button>
        {#if chats.length}<a class="meta" href="#/search" style:display="inline-flex" style:align-items="center" style:min-height="44px">1–{recent.length} of {chats.length.toLocaleString()}</a>{/if}
      </span>
    </div>
    {#if catalogue.isPending}<Skeleton rows={8} />{/if}
    {#each recent as chat (chat.machine + chat.id)}<ChatRow {chat} />{/each}
    {#if catalogue.data && !chats.length}<p class="meta">No indexed chats yet. Run <code>mash index</code>, then reload.</p>{/if}
    {#each catalogue.data?.warnings ?? [] as warning (warning)}<p class="meta" role="status">{warning}</p>{/each}
  </section>
  <div class="c-side">
    <section>
      <h2 class="h2"><a href="#/records?"><BookOpen size={15} aria-hidden="true" />recent memories</a></h2>
      {#if memories.isPending}<Skeleton rows={3} />{/if}
      {#each (memories.data?.records ?? []).slice(0, 4) as record (record.node + record.id)}<MemoryRow {record} />{/each}
      {#if memories.data && !memories.data.records.length}<p class="meta">Nothing saved yet. Agents save decisions and preferences as you state them.</p>{/if}
      <QueryError title="Couldn’t load memory" query={memories} />
    </section>
    <section>
      <h2 class="h2"><a href="#/machines"><Laptop size={15} aria-hidden="true" />machines</a></h2>
      {#each machines as machine (machine.key)}
        <a class="li" href={machineHref(machine.key)}><i class="dot"></i><Laptop size={16} aria-hidden="true" /><span class="g">{machine.key}</span><span class="n">{machine.chats.length.toLocaleString()}</span></a>
      {/each}
    </section>
    <section>
      <h2 class="h2"><a href="#/agents"><Bot size={15} aria-hidden="true" />agents</a></h2>
      {#each providers as provider (provider.key)}
        {@const agent = provider.chats[0]?.agent}
        {#if agent}<a class="li" href={agentHref(agent)}><AgentMark {agent} size={14} /><span class="g">{agentInfo[agent].name}</span><span class="n">{provider.chats.length.toLocaleString()}</span></a>{/if}
      {/each}
    </section>
    <a class="meta" href="#/services" style:display="inline-flex" style:align-items="center" style:min-height="44px">services and machine health</a>
  </div>
</div>
