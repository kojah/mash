<script lang="ts">
import Bot from "@lucide/svelte/icons/bot";
import ChevronRight from "@lucide/svelte/icons/chevron-right";
import Clock from "@lucide/svelte/icons/clock";
import Laptop from "@lucide/svelte/icons/laptop";
import Search from "@lucide/svelte/icons/search";
import { agents as agentInfo } from "../lib/agents";
import { type Overview, overview } from "../lib/api";
import { chatHref } from "../lib/route.svelte";
import AgentMark from "./AgentMark.svelte";
import SearchPanel from "./SearchPanel.svelte";

let data = $state<Overview | null>(null);
let searching = $state(false);

$effect(() => {
  overview().then((loaded) => {
    data = loaded;
  });
});

function onKey(event: KeyboardEvent): void {
  if (event.key === "Escape") searching = false;
}
</script>

<svelte:window onkeydown={onKey} />

<main class="mx-auto flex max-w-[600px] flex-col gap-9 px-5 py-16">
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

  {#if data}
    <section aria-labelledby="recent" class="flex flex-col gap-2">
      <h2 id="recent" class="m-0 flex items-center gap-2 text-[13px] font-normal text-muted"><Clock size={15} strokeWidth={1.75} />Recent</h2>
      <ul class="m-0 flex list-none flex-col overflow-hidden rounded-xl bg-surface p-0">
        {#each data.recent as chat (chat.id)}
          <li>
            <a href={chatHref(chat.id)} class="row text-[15px]">
              <AgentMark agent={chat.agent} />
              <span class="min-w-0 flex-1 truncate">{chat.title}</span>
              <span class="shrink-0 text-[13px] tabular-nums text-faint">{chat.updatedAt}</span>
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
          {#each data.machines as machine (machine.name)}
            <li>
              <a href="#/" class="row text-[15px]" aria-label="{machine.name}{machine.needsAttention ? ', needs attention' : ''}">
                <Laptop size={18} strokeWidth={1.75} class="shrink-0 text-ink-2" />
                <span class="flex-1">{machine.name}</span>
                {#if machine.needsAttention}<span aria-hidden="true" class="size-2 rounded-full bg-warn"></span>{/if}
                <ChevronRight size={14} strokeWidth={2.25} class="shrink-0 text-chevron" />
              </a>
            </li>
          {/each}
        </ul>
      </section>

      <section aria-labelledby="agents" class="flex flex-col gap-2">
        <h2 id="agents" class="m-0 flex items-center gap-2 text-[13px] font-normal text-muted"><Bot size={15} strokeWidth={1.75} />Agents</h2>
        <ul class="m-0 flex list-none flex-col overflow-hidden rounded-xl bg-surface p-0">
          {#each data.agents as summary (summary.agent)}
            <li>
              <a href="#/" class="row text-[15px]" aria-label="{agentInfo[summary.agent].name}{summary.quiet ? `, quiet for ${summary.lastChat}` : ''}">
                <AgentMark agent={summary.agent} />
                <span class="min-w-0 flex-1 truncate">{agentInfo[summary.agent].name}</span>
                {#if summary.quiet}<span aria-hidden="true" class="size-2 rounded-full bg-[#aeaeb2]"></span>{/if}
                <ChevronRight size={14} strokeWidth={2.25} class="shrink-0 text-chevron" />
              </a>
            </li>
          {/each}
        </ul>
      </section>
    </div>
  {/if}
</main>

{#if searching}
  <button type="button" aria-label="Close search" class="fixed inset-0 z-10 cursor-default border-0 bg-black/30" onclick={() => (searching = false)}></button>
  <div role="dialog" aria-modal="true" aria-label="Search" class="fixed top-[12vh] left-1/2 z-20 flex max-h-[76vh] w-[min(720px,calc(100vw-32px))] -translate-x-1/2 flex-col overflow-hidden rounded-[18px] bg-ground p-6 shadow-[0_24px_60px_rgba(0,0,0,0.2)]">
    <SearchPanel onClose={() => (searching = false)} />
  </div>
{/if}
