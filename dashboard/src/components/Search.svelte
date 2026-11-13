<script lang="ts">
import Bot from "@lucide/svelte/icons/bot";
import ChevronLeft from "@lucide/svelte/icons/chevron-left";
import ChevronRight from "@lucide/svelte/icons/chevron-right";
import Clock from "@lucide/svelte/icons/clock";
import ClockArrowDown from "@lucide/svelte/icons/clock-arrow-down";
import ClockArrowUp from "@lucide/svelte/icons/clock-arrow-up";
import Folder from "@lucide/svelte/icons/folder";
import Laptop from "@lucide/svelte/icons/laptop";
import SearchIcon from "@lucide/svelte/icons/search";
import SlidersHorizontal from "@lucide/svelte/icons/sliders-horizontal";
import Sparkles from "@lucide/svelte/icons/sparkles";
import Terminal from "@lucide/svelte/icons/terminal";
import { createQuery } from "@tanstack/svelte-query";
import { untrack } from "svelte";
import { agents as agentInfo } from "../lib/agents";
import { browse, isAgent, search } from "../lib/api";
import { groupChats, projectLabel } from "../lib/browse";
import { navigate } from "../lib/navigation";
import { catalogOptions, stableRead } from "../lib/queries";
import { chatHref } from "../lib/route.svelte";
import {
  type Applied,
  appliedHref,
  dateRanges,
  filtersOf,
  parseApplied,
  rankings,
  type Sort,
  sameSearch,
  sorts,
} from "../lib/search";
import AgentMark from "./AgentMark.svelte";
import FilterMenu from "./FilterMenu.svelte";
import Highlight from "./Highlight.svelte";
import MetaChips from "./MetaChips.svelte";
import QueryError from "./QueryError.svelte";

// Chats and search are one page: without a query it lists chats, newest
// first. The address is the applied search; the chips below are a draft
// until "Apply filters" is chosen, so results never change under the reader.
let { params }: { params: string } = $props();
const pageSize = 10;
const applied = $derived(parseApplied(params));
let draft = $state<Applied>(untrack(() => parseApplied(params)));
const dirty = $derived(!sameSearch(draft, applied));
const catalogue = createQuery(() => catalogOptions);
const chats = $derived(catalogue.data?.chats ?? []);

const options = $derived({
  agent: groupChats(chats, (chat) => chat.agent).map((group) => ({
    value: group.key,
    label: isAgent(group.key) ? agentInfo[group.key].name : group.key,
    count: group.chats.length,
  })),
  machine: groupChats(chats, (chat) => chat.machine).map((group) => ({
    value: group.key,
    label: group.key,
    count: group.chats.length,
  })),
  project: groupChats(
    chats.filter((chat) => chat.project),
    (chat) => chat.project,
  ).map((group) => ({
    value: group.key,
    label: projectLabel(group.key),
    count: group.chats.length,
  })),
});

const result = createQuery(() => ({
  ...stableRead,
  // A list of chats without a question follows the chat list as it updates;
  // a submitted search keeps its results until it is submitted again.
  queryKey: ["search", params, applied.query ? 0 : catalogue.dataUpdatedAt],
  placeholderData: (previous, query) => (query?.queryKey[1] === params ? previous : undefined),
  enabled: catalogue.data !== undefined,
  queryFn: async ({ signal }: { signal: AbortSignal }) => {
    const data = catalogue.data;
    if (!data) throw new Error("The chat list is not loaded yet.");
    if (applied.query) {
      const page = await search(
        applied.query,
        filtersOf(applied),
        applied.offset,
        data.localNode,
        signal,
        applied.rank,
      );
      return { ...page, total: null, first: applied.offset };
    }
    const all = browse(data, filtersOf(applied), Date.now());
    const ordered = applied.sort === "old" ? [...all.hits].reverse() : all.hits;
    // A link to a page past the end, say after chats were forgotten, shows
    // the last page instead of claiming nothing matched.
    const lastPage = Math.max(0, Math.floor((ordered.length - 1) / pageSize) * pageSize);
    const first = Math.min(applied.offset, lastPage);
    return {
      hits: ordered.slice(first, first + pageSize),
      nextOffset: first + pageSize < ordered.length ? first + pageSize : null,
      warnings: all.warnings,
      total: ordered.length,
      first,
    };
  },
}));
const hits = $derived(result.data?.hits ?? []);

function toggle(field: "agents" | "machines" | "projects", value: string): void {
  const current: readonly string[] = draft[field];
  const next = current.includes(value)
    ? current.filter((item) => item !== value)
    : [...current, value];
  draft =
    field === "agents" ? { ...draft, agents: next.filter(isAgent) } : { ...draft, [field]: next };
}
function chipLabel(field: "agents" | "machines" | "projects", noun: string): string {
  const chosen = draft[field];
  if (chosen.length === 0) return noun;
  if (chosen.length > 1) return `${chosen.length} ${noun}s`;
  const [only] = chosen;
  if (only === undefined) return noun;
  if (field === "agents") return isAgent(only) ? agentInfo[only].name : only;
  return field === "projects" ? projectLabel(only) : only;
}
const changed = (field: "agents" | "machines" | "projects") =>
  [...draft[field]].sort().join() !== [...applied[field]].sort().join();
function chooseSort(value: string): void {
  const sort: Sort | undefined = sorts.find((option) => option.value === value)?.value;
  if (sort) draft = { ...draft, sort };
}
function apply(): void {
  navigate(appliedHref({ ...draft, query: applied.query, offset: 0 }));
}
function page(offset: number): string {
  return appliedHref({ ...applied, offset });
}
const rangeLabel = $derived.by(() => {
  if (!result.data) return "";
  const first = result.data.first + 1;
  const last = result.data.first + hits.length;
  if (result.data.total !== null)
    return result.data.total > pageSize
      ? `${first.toLocaleString()}–${last.toLocaleString()} of ${result.data.total.toLocaleString()} chats`
      : `${result.data.total.toLocaleString()} chat${result.data.total === 1 ? "" : "s"}`;
  return `results ${first.toLocaleString()}–${last.toLocaleString()} for “${applied.query}”`;
});
</script>

<h1 class="sr-only">{applied.query ? "Search results" : "Chats"}</h1>
<div class="chips" role="group" aria-label="Filters">
  <FilterMenu label={chipLabel("agents", "agent")} active={draft.agents.length > 0} draft={changed("agents")} multiple options={options.agent} selected={draft.agents} choose={(value) => toggle("agents", value)}>
    {#snippet icon()}<Bot size={15} aria-hidden="true" />{/snippet}
    {#snippet optionIcon(value: string)}{#if isAgent(value)}<AgentMark agent={value} size={15} />{:else}<Bot size={15} aria-hidden="true" />{/if}{/snippet}
  </FilterMenu>
  <FilterMenu label={chipLabel("machines", "machine")} active={draft.machines.length > 0} draft={changed("machines")} multiple options={options.machine} selected={draft.machines} choose={(value) => toggle("machines", value)}>
    {#snippet icon()}<Laptop size={15} aria-hidden="true" />{/snippet}
  </FilterMenu>
  <FilterMenu label={chipLabel("projects", "project")} active={draft.projects.length > 0} draft={changed("projects")} multiple options={options.project} selected={draft.projects} choose={(value) => toggle("projects", value)}>
    {#snippet icon()}<Folder size={15} aria-hidden="true" />{/snippet}
  </FilterMenu>
  <FilterMenu
    label={dateRanges.find((range) => range.days === draft.days)?.label ?? "any time"}
    active={draft.days !== null}
    draft={draft.days !== applied.days}
    options={dateRanges.map((range) => ({ value: String(range.days ?? ""), label: range.label }))}
    selected={[String(draft.days ?? "")]}
    choose={(value) => (draft = { ...draft, days: value ? Number(value) : null })}
  >
    {#snippet icon()}<Clock size={15} aria-hidden="true" />{/snippet}
  </FilterMenu>
  {#if !applied.query}
    <FilterMenu
      label="sort: {sorts.find((sort) => sort.value === draft.sort)?.label}"
      draft={draft.sort !== applied.sort}
      options={sorts}
      selected={[draft.sort]}
      choose={chooseSort}
    >
      {#snippet icon()}<SlidersHorizontal size={15} aria-hidden="true" />{/snippet}
      {#snippet optionIcon(value: string)}{#if value === "old"}<ClockArrowUp size={15} aria-hidden="true" />{:else}<ClockArrowDown size={15} aria-hidden="true" />{/if}{/snippet}
    </FilterMenu>
  {:else}
    <FilterMenu
      label="sort: {rankings.find((ranking) => ranking.value === draft.rank)?.label}"
      draft={draft.rank !== applied.rank}
      options={rankings}
      selected={[draft.rank]}
      choose={(value) => (draft = { ...draft, rank: value === "recent" ? "recent" : "relevance" })}
    >
      {#snippet icon()}<SlidersHorizontal size={15} aria-hidden="true" />{/snippet}
      {#snippet optionIcon(value: string)}{#if value === "recent"}<ClockArrowDown size={15} aria-hidden="true" />{:else}<Sparkles size={15} aria-hidden="true" />{/if}{/snippet}
    </FilterMenu>
  {/if}
  {#if dirty}<button class="a" id="applyf" type="button" onclick={apply} style:min-height="30px" style:padding="0 6px" style:margin-left="6px" style:font-size="13.5px">Apply filters</button>{/if}
</div>

<QueryError title="Couldn’t load chats" query={catalogue} />
<QueryError title="Couldn’t load results" query={result} />
{#if (result.isPending || catalogue.isPending) && !result.error && !catalogue.error}
  <div class="state-c" role="status">
    <p style:display="flex" style:align-items="center" style:gap="10px"><Terminal size={18} color="var(--clay2)" aria-hidden="true" />{applied.query ? "Searching…" : "Loading chats…"}</p>
    <div class="skel" style:width="82%"></div><div class="skel" style:width="64%"></div><div class="skel" style:width="74%"></div>
  </div>
{/if}
{#if result.data}
  {#each result.data.warnings as warning (warning)}<p class="meta" role="status">{warning}</p>{/each}
  {#if !hits.length}
    <div class="state-c">
      <h2 style:font-size="15px" style:display="flex" style:align-items="center" style:gap="8px"><SearchIcon size={17} aria-hidden="true" />No results</h2>
      <p class="meta" style:margin="4px 0 12px" style:font-size="13.5px">{applied.query ? `Nothing matched “${applied.query}” with these filters.` : "Nothing matched these filters."}</p>
      <a class="btn" href="#/search">Clear filters</a>
    </div>
  {:else}
    <section id="reslist">
      <p class="meta" style:font-size="13.5px" role="status">{rangeLabel}</p>
      {#each hits as hit (JSON.stringify([hit.chat.machine, hit.chat.id, hit.messageId]))}
        <a class="res" href={chatHref(hit.chat.id, hit.chat.machine, hit.messageId)}>
          <span class="res-t"><AgentMark agent={hit.chat.agent} /><span style:min-width="0"><Highlight text={hit.chat.title} query={applied.query} /></span></span>
          {#if hit.before}<span class="res-s"><Highlight text={hit.before} query={applied.query} /></span>{/if}
          <span class="elb" style:margin-top="5px"><MetaChips chat={hit.chat} /></span>
        </a>
      {/each}
      {#if result.data.first > 0 || result.data.nextOffset !== null}
        <nav class="hd" aria-label="Pages" style:margin-top="14px">
          <span class="meta">{rangeLabel}</span>
          <span class="acts">
            {#if result.data.first > 0}<a class="btn" rel="prev" href={page(Math.max(0, result.data.first - pageSize))}><ChevronLeft size={15} aria-hidden="true" />Previous</a>{:else}<button class="btn" type="button" disabled><ChevronLeft size={15} aria-hidden="true" />Previous</button>{/if}
            {#if result.data.nextOffset !== null}<a class="btn" rel="next" href={page(result.data.nextOffset)}>Next<ChevronRight size={15} aria-hidden="true" /></a>{:else}<button class="btn" type="button" disabled>Next<ChevronRight size={15} aria-hidden="true" /></button>{/if}
          </span>
        </nav>
      {/if}
    </section>
  {/if}
{/if}
