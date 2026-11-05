<script lang="ts">
import Check from "@lucide/svelte/icons/check";
import ChevronDown from "@lucide/svelte/icons/chevron-down";
import Search from "@lucide/svelte/icons/search";
import X from "@lucide/svelte/icons/x";
import { agents as agentInfo } from "../lib/agents";
import { chats, search } from "../lib/api";
import { chatHref } from "../lib/route.svelte";
import type { Agent, ChatSummary, SearchHit } from "../lib/types";
import AgentMark from "./AgentMark.svelte";

let { onClose, compact = false }: { onClose: () => void; compact?: boolean } = $props();

type Facet = "agents" | "machines" | "projects";
const dates = [
  { label: "Any time", days: null },
  { label: "Today", days: 1 },
  { label: "7 days", days: 7 },
  { label: "30 days", days: 30 },
] as const;

let query = $state("");
let selected = $state<{ agents: Agent[]; machines: string[]; projects: string[] }>({
  agents: [],
  machines: [],
  projects: [],
});
let days = $state<number | null>(null);
let openFacet = $state<Facet | null>(null);
let results = $state<readonly SearchHit[] | null>(null);
let catalogue = $state<readonly ChatSummary[]>([]);

$effect(() => {
  chats().then((all) => {
    catalogue = all;
  });
});

const options = $derived({
  agents: [...new Set(catalogue.map((chat) => chat.agent))],
  machines: [...new Set(catalogue.map((chat) => chat.machine))],
  projects: [...new Set(catalogue.map((chat) => chat.project))],
});

const tags = $derived([
  ...selected.agents.map((value) => ({
    facet: "agents" as const,
    value,
    label: agentInfo[value].name,
  })),
  ...selected.machines.map((value) => ({ facet: "machines" as const, value, label: value })),
  ...selected.projects.map((value) => ({ facet: "projects" as const, value, label: value })),
  ...(days === null
    ? []
    : [
        {
          facet: "days" as const,
          value: String(days),
          label: dates.find((d) => d.days === days)?.label ?? "",
        },
      ]),
]);

function toggle(facet: Facet, value: string): void {
  const list = selected[facet] as string[];
  selected[facet] = (
    list.includes(value) ? list.filter((v) => v !== value) : [...list, value]
  ) as never;
}

function removeTag(facet: Facet | "days", value: string): void {
  if (facet === "days") days = null;
  else toggle(facet, value);
}

function clearAll(): void {
  selected = { agents: [], machines: [], projects: [] };
  days = null;
}

async function run(): Promise<void> {
  openFacet = null;
  results = await search(query, { ...selected, days });
}

function label(facet: Facet): string {
  const names = { agents: "Agent", machines: "Machine", projects: "Project" } as const;
  const chosen = selected[facet];
  if (chosen.length === 0) return names[facet];
  const first = facet === "agents" ? agentInfo[chosen[0] as Agent].name : chosen[0];
  return chosen.length === 1 ? `${names[facet]}: ${first}` : `${names[facet]}: ${chosen.length}`;
}

const chip =
  "inline-flex min-h-8 items-center gap-1 rounded-full border px-2.5 text-[13px] cursor-pointer";
</script>

<div class="flex flex-col gap-5" class:gap-4={compact}>
  <div role="search" class="flex items-center gap-2.5">
    <div class="flex min-h-[52px] min-w-0 flex-1 flex-wrap items-center gap-1.5 rounded-xl border border-line bg-surface py-1.5 pr-2 pl-3">
      {#each tags as tag (tag.facet + tag.value)}
        <span class="inline-flex min-h-[30px] items-center gap-1 rounded-lg bg-accent-soft py-0 pr-1 pl-2.5 text-sm whitespace-nowrap text-accent-ink">
          {tag.label}
          <button type="button" class="inline-flex size-6 items-center justify-center rounded-md" aria-label="Remove filter {tag.label}" onclick={() => removeTag(tag.facet, tag.value)}>
            <X size={12} strokeWidth={2.5} />
          </button>
        </span>
      {/each}
      <label for="search-query" class="sr-only">Search your agent chats</label>
      <!-- svelte-ignore a11y_autofocus -->
      <input
        id="search-query"
        type="search"
        placeholder="Search chats"
        autofocus
        bind:value={query}
        onkeydown={(event) => event.key === "Enter" && run()}
        class="min-h-10 min-w-[120px] flex-1 border-0 bg-transparent px-1 text-[17px] text-ink outline-none"
      />
      {#if tags.length > 0}
        <button type="button" aria-label="Clear all filters" onclick={clearAll} class="inline-flex size-[30px] shrink-0 items-center justify-center rounded-full bg-hairline text-muted">
          <X size={12} strokeWidth={2.5} />
        </button>
      {/if}
    </div>
    <button type="button" aria-label="Search" onclick={run} class="inline-flex size-[52px] shrink-0 items-center justify-center rounded-xl bg-accent text-white">
      <Search size={20} strokeWidth={2} />
    </button>
  </div>

  <div role="group" aria-label="Filters" class="flex flex-wrap items-center gap-2 px-0.5">
    {#each ["agents", "machines", "projects"] as const as facet (facet)}
      {@const active = selected[facet].length > 0}
      <div class="relative">
        <button
          type="button"
          aria-haspopup="true"
          aria-expanded={openFacet === facet}
          onclick={() => (openFacet = openFacet === facet ? null : facet)}
          class="{chip} {active ? 'border-[#9cc3f5] bg-accent-soft text-accent-ink' : 'border-line bg-surface text-ink'}"
        >
          {label(facet)}<ChevronDown size={13} strokeWidth={2} />
        </button>
        {#if openFacet === facet}
          <ul class="absolute top-10 left-0 z-10 m-0 min-w-[200px] list-none rounded-[10px] bg-surface p-1.5 shadow-[0_12px_32px_rgba(0,0,0,0.16),0_0_0_0.5px_rgba(0,0,0,0.08)]">
            {#each options[facet] as value (value)}
              {@const on = (selected[facet] as string[]).includes(value)}
              <li>
                <button type="button" role="menuitemcheckbox" aria-checked={on} onclick={() => toggle(facet, value)} class="flex min-h-10 w-full items-center gap-2.5 rounded-md px-2 text-left text-sm text-ink hover:bg-ground">
                  <span aria-hidden="true" class="inline-flex size-4 items-center justify-center rounded border-[1.5px] {on ? 'border-accent bg-accent text-white' : 'border-[#b8bcc5] bg-surface'}">
                    {#if on}<Check size={12} strokeWidth={3} />{/if}
                  </span>
                  <span class="flex-1">{facet === "agents" ? agentInfo[value as Agent].name : value}</span>
                </button>
              </li>
            {/each}
          </ul>
        {/if}
      </div>
    {/each}
    <span aria-hidden="true" class="mx-1 h-5 w-px bg-line"></span>
    {#each dates as date (date.label)}
      {@const on = days === date.days && date.days !== null}
      <button type="button" aria-pressed={days === date.days} onclick={() => (days = date.days)} class="{chip} {on ? 'border-[#9cc3f5] bg-accent-soft text-accent-ink' : 'border-line bg-surface text-ink'}">
        {date.label}
      </button>
    {/each}
  </div>

  {#if results === null}
    <p class="m-0 py-8 text-center text-sm text-faint">Press Return to search</p>
  {:else}
    <section aria-label="Search results" class="flex min-h-0 flex-col gap-2 overflow-y-auto">
      <p class="m-0 text-[13px] text-muted">
        {results.length} chat{results.length === 1 ? "" : "s"}{query.trim() === "" ? " · newest first" : ""}
      </p>
      <ul class="m-0 flex list-none flex-col overflow-hidden rounded-xl bg-surface p-0">
        {#each results as hit (hit.chat.id + hit.messageId)}
          <li>
            <a href={chatHref(hit.chat.id)} onclick={onClose} class="row items-start py-3">
              <AgentMark agent={hit.chat.agent} />
              <span class="flex min-w-0 flex-1 flex-col gap-1">
                <span class="text-[15px] font-medium">{hit.chat.title}</span>
                {#if hit.match}
                  <span class="line-clamp-2 text-sm leading-snug text-ink-2">{hit.before}<mark class="rounded-sm bg-[#fff1b8] text-inherit">{hit.match}</mark>{hit.after}</span>
                {/if}
                <span class="text-xs text-faint">{agentInfo[hit.chat.agent].name} · {hit.chat.machine} · {hit.chat.updatedAt}</span>
              </span>
            </a>
          </li>
        {/each}
      </ul>
    </section>
  {/if}
</div>
