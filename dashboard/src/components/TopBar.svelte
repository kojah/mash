<script lang="ts">
import Search from "@lucide/svelte/icons/search";
import { createQuery } from "@tanstack/svelte-query";
import { agents as agentInfo } from "../lib/agents";
import { groupChats, projectLabel } from "../lib/browse";
import { navigate } from "../lib/navigation";
import { catalogOptions } from "../lib/queries";
import { chatHref, router, searchHref } from "../lib/route.svelte";
import { type Applied, appliedHref, dateRanges, parseApplied } from "../lib/search";
import AgentMark from "./AgentMark.svelte";

let { railOpen, toggleRail }: { railOpen: boolean; toggleRail: () => void } = $props();
const catalogue = createQuery(() => catalogOptions);
let text = $state("");
let focused = $state(false);
let root: HTMLDivElement;
// The box shows the submitted search; typing is a draft until it is submitted.
$effect(() => {
  const route = router.current;
  text = route.page === "search" ? (new URLSearchParams(route.params).get("q") ?? "") : "";
});
// Applied filters show as tags in the box; removing one reruns the search.
const applied = $derived(
  router.current.page === "search" ? parseApplied(router.current.params) : null,
);
const tags = $derived.by(() => {
  if (!applied) return [];
  const without = (change: Partial<Applied>) => appliedHref({ ...applied, ...change, offset: 0 });
  return [
    ...applied.agents.map((agent) => ({
      label: agentInfo[agent].name,
      href: without({ agents: applied.agents.filter((item) => item !== agent) }),
    })),
    ...applied.machines.map((machine) => ({
      label: machine,
      href: without({ machines: applied.machines.filter((item) => item !== machine) }),
    })),
    ...applied.projects.map((project) => ({
      label: projectLabel(project),
      href: without({ projects: applied.projects.filter((item) => item !== project) }),
    })),
    ...(applied.days === null
      ? []
      : [
          {
            label: dateRanges.find((range) => range.days === applied.days)?.label ?? "date",
            href: without({ days: null }),
          },
        ]),
  ];
});
const needle = $derived(text.trim().toLowerCase());
const suggestions = $derived.by(() => {
  const chats = catalogue.data?.chats ?? [];
  if (needle.length < 2) return null;
  const has = (value: string) => value.toLowerCase().includes(needle);
  const filters = [
    ...groupChats(chats, (chat) => chat.agent)
      .filter((group) => has(agentInfo[group.chats[0]?.agent ?? "codex"].name))
      .map((group) => ({ kind: "agent", value: group.key, label: group.key })),
    ...groupChats(chats, (chat) => chat.machine)
      .filter((group) => has(group.key))
      .map((group) => ({ kind: "machine", value: group.key, label: group.key })),
    ...groupChats(
      chats.filter((chat) => chat.project),
      (chat) => chat.project,
    )
      .filter((group) => has(projectLabel(group.key)))
      .map((group) => ({ kind: "project", value: group.key, label: projectLabel(group.key) })),
  ].slice(0, 5);
  return { filters, chats: chats.filter((chat) => has(chat.title)).slice(0, 4) };
});
const showDrop = $derived(
  focused && suggestions !== null && suggestions.filters.length + suggestions.chats.length > 0,
);
function submit(event: SubmitEvent): void {
  event.preventDefault();
  const route = router.current;
  const params = new URLSearchParams(route.page === "search" ? route.params : "");
  const query = text.trim();
  if (query) params.set("q", query);
  else params.delete("q");
  // A new query is a new search: it starts at the first result.
  params.delete("offset");
  focused = false;
  navigate(params.size ? `#/search?${params}` : "#/search");
}
</script>

<header id="top">
  <button id="burger" type="button" aria-label="Open navigation" aria-expanded={railOpen} onclick={toggleRail}><img src="/mash-logo.png" alt="" width="30" height="28" /></button>
  <div
    class="search"
    role="search"
    bind:this={root}
    onfocusin={() => (focused = true)}
    onfocusout={(event) => {
      if (!(event.relatedTarget instanceof Node) || !root.contains(event.relatedTarget)) focused = false;
    }}
  >
    <form class="box" class:open={showDrop} onsubmit={submit}>
      {#each tags as tag (tag.label + tag.href)}<a class="tag" href={tag.href} aria-label="Remove filter {tag.label}" style:display="inline-flex" style:align-items="center">{tag.label} ×</a>{/each}
      <label for="q" class="sr-only">Search your history</label>
      <input id="q" autocomplete="off" placeholder="Search your history" bind:value={text} onkeydown={(event) => { if (event.key === "Escape") focused = false; }} />
      <button class="go" type="submit" aria-label="Search"><Search size={16} /></button>
    </form>
    {#if showDrop && suggestions}
      <div class="drop">
        {#if suggestions.filters.length}
          <h4>filters</h4>
          {#each suggestions.filters as filter (filter.kind + filter.value)}
            <a class="opt" href={searchHref({ [filter.kind]: filter.value })} onclick={() => (focused = false)}><span class="t"><span>{filter.label}</span></span><small>{filter.kind}</small></a>
          {/each}
        {/if}
        {#if suggestions.chats.length}
          <h4>chats</h4>
          {#each suggestions.chats as chat (chat.machine + chat.id)}
            <a class="opt" href={chatHref(chat.id, chat.machine)} onclick={() => (focused = false)}><AgentMark agent={chat.agent} size={14} /><span class="t"><span>{chat.title}</span></span></a>
          {/each}
        {/if}
      </div>
    {/if}
  </div>
</header>
