<script lang="ts">
import BadgeCheck from "@lucide/svelte/icons/badge-check";
import CircleHelp from "@lucide/svelte/icons/circle-help";
import Database from "@lucide/svelte/icons/database";
import Gavel from "@lucide/svelte/icons/gavel";
import ListChecks from "@lucide/svelte/icons/list-checks";
import SlidersHorizontal from "@lucide/svelte/icons/sliders-horizontal";
import { createQuery } from "@tanstack/svelte-query";
import { untrack } from "svelte";
import { whenLabel } from "../lib/browse";
import { navigate } from "../lib/navigation";
import { catalogOptions, stableRead } from "../lib/queries";
import { kinds, label, recordPage, scopeLabel, states, timingLabel } from "../lib/records";
import { recordHref, recordsHref } from "../lib/route.svelte";
import FilterMenu from "./FilterMenu.svelte";
import QueryError from "./QueryError.svelte";
import Skeleton from "./Skeleton.svelte";

const icons = {
  decision: Gavel,
  preference: SlidersHorizontal,
  fact: BadgeCheck,
  open_question: CircleHelp,
} as const;
const now = Date.now();

let { params }: { params: string } = $props();
const initial = untrack(() => new URLSearchParams(params));
let query = $state(initial.get("q") ?? "");
let kind = $state(initial.get("kind") ?? "");
let lifecycle = $state(initial.get("state") ?? "");
let review = $state(initial.get("review") ?? "");
let source = $state(
  initial.get("shared") === "true"
    ? initial.get("node")
      ? `node:${initial.get("node")}`
      : "shared"
    : "",
);
const chat = initial.get("chat") ?? "";
const catalogue = createQuery(() => catalogOptions);
const nodes = $derived([
  ...new Set([
    ...(catalogue.data?.chats.map((chat) => chat.machine) ?? []),
    ...(initial.get("node") ? [initial.get("node") ?? ""] : []),
  ]),
]);
const page = createQuery(() => ({
  ...stableRead,
  queryKey: ["records", params],
  queryFn: ({ signal }) => recordPage(params, signal),
}));
const hasNext = $derived(
  page.data && (page.data.nextCursor !== null || page.data.nextOffset !== null),
);
function submit(event: SubmitEvent): void {
  event.preventDefault();
  const next = new URLSearchParams();
  if (query.trim()) next.set("q", query.trim());
  if (kind) next.set("kind", kind);
  if (lifecycle) next.set("state", lifecycle);
  if (review) next.set("review", review);
  if (chat) next.set("chat", chat);
  if (source) {
    next.set("shared", "true");
    if (source.startsWith("node:")) next.set("node", source.slice(5));
  }
  navigate(recordsHref(next));
}
function nextPage(): void {
  if (!page.data) return;
  const next = new URLSearchParams(params);
  next.delete("offset");
  next.delete("cursor_node");
  next.delete("cursor_offset");
  if (page.data.nextCursor) {
    next.set("cursor_node", page.data.nextCursor.node);
    next.set("cursor_offset", String(page.data.nextCursor.offset));
  } else if (page.data.nextOffset !== null) next.set("offset", String(page.data.nextOffset));
  navigate(recordsHref(next));
}
function firstPage(): void {
  const next = new URLSearchParams(params);
  next.delete("offset");
  next.delete("cursor_node");
  next.delete("cursor_offset");
  navigate(recordsHref(next));
}
function kindHref(value: string): string {
  const next = new URLSearchParams(params);
  next.delete("offset");
  next.delete("cursor_node");
  next.delete("cursor_offset");
  if (value) next.set("kind", value);
  else next.delete("kind");
  return recordsHref(next);
}
// The chips are a draft until the form is submitted.
const dirty = $derived(
  query.trim() !== (initial.get("q") ?? "") ||
    lifecycle !== (initial.get("state") ?? "") ||
    review !== (initial.get("review") ?? "") ||
    source !==
      (initial.get("shared") === "true"
        ? initial.get("node")
          ? `node:${initial.get("node")}`
          : "shared"
        : ""),
);
</script>

<div><h1 class="h1">Memory</h1><p class="meta">Decisions, preferences, facts and open questions, with the conversations behind them.</p></div>
<nav class="chips" aria-label="Memory kind">
  <a class="chip big-c" class:on={!kind} href={kindHref("")} aria-current={!kind ? "page" : undefined}>all</a>
  {#each kinds as value (value)}
    {@const Icon = icons[value]}
    <a class="chip big-c" class:on={kind === value} href={kindHref(value)} aria-current={kind === value ? "page" : undefined}><Icon size={15} color="var(--pink)" aria-hidden="true" />{label(value).toLowerCase()}s</a>
  {/each}
</nav>
<form onsubmit={submit} class="chips" aria-label="Memory filters">
  <label class="sr-only" for="memory-q">Search statements</label>
  <input id="memory-q" class="field" type="search" bind:value={query} placeholder="find a statement" style:flex="1 1 220px" style:max-width="360px" />
  <FilterMenu label={lifecycle ? label(lifecycle).toLowerCase() : "any status"} active={!!lifecycle} draft={lifecycle !== (initial.get("state") ?? "")} options={[{ value: "", label: "any status" }, ...states.map((value) => ({ value, label: label(value).toLowerCase() }))]} selected={[lifecycle]} choose={(value) => (lifecycle = value)}>
    {#snippet icon()}<ListChecks size={15} aria-hidden="true" />{/snippet}
  </FilterMenu>
  <FilterMenu label={review || "any review"} active={!!review} draft={review !== (initial.get("review") ?? "")} options={[{ value: "", label: "any review" }, { value: "established", label: "established" }, { value: "candidate", label: "candidate (unconfirmed)" }]} selected={[review]} choose={(value) => (review = value)}>
    {#snippet icon()}<BadgeCheck size={15} aria-hidden="true" />{/snippet}
  </FilterMenu>
  <FilterMenu label={source === "" ? "this machine" : source === "shared" ? "all shared" : source.slice(5)} active={!!source} options={[{ value: "", label: "this machine" }, { value: "shared", label: "all shared machines" }, ...nodes.map((node) => ({ value: `node:${node}`, label: `${node} (shared)` }))]} selected={[source]} choose={(value) => (source = value)}>
    {#snippet icon()}<Database size={15} aria-hidden="true" />{/snippet}
  </FilterMenu>
  {#if dirty}<button class="a" id="applyf" type="submit" style:min-height="30px" style:padding="0 6px" style:font-size="12.5px">Apply filters</button>{/if}
  <button class="a meta" type="button" disabled={page.isFetching} onclick={() => page.refetch()} style:min-height="30px" style:margin-left="auto">{page.isFetching ? "checking…" : "check for updates"}</button>
</form>
{#if chat}<p class="meta">Memory citing chat <span style:overflow-wrap="anywhere">{chat}</span>. <a class="a" href="#/records?">Show all memory</a></p>{/if}
{#if page.isPending}<Skeleton rows={5} />{/if}
<QueryError title="Couldn’t load memory" query={page} />
{#if page.data && !page.error}
  {#if !page.data.records.length}
    <div class="state-c"><p>{hasNext ? "No matching records on this page. Continue to the next page." : "No memory matches these filters. Agents save decisions, preferences and facts as you state them."}</p></div>
  {/if}
  <section>
    {#each page.data.records as record (JSON.stringify([record.node, record.owner, record.id]))}
      {@const revision = record.revisions.find((revision) => revision.revision === record.currentRevision)}
      {#if revision}
        {@const Icon = icons[revision.details.kind]}
        <article style:padding="14px 0" style:border-bottom="1px solid var(--hair)">
          <p style:color="var(--pink)" style:font-size="12px" style:display="flex" style:flex-wrap="wrap" style:align-items="center" style:gap="6px">
            <Icon size={15} aria-hidden="true" />{label(revision.details.kind).toLowerCase()} / {label(revision.details.state).toLowerCase()}{revision.review === "candidate" ? " / unconfirmed" : ""}
            {#if !revision.evidenceCurrent}<span class="warn">/ stale evidence</span>{/if}
            {#if record.conflicted}<span class="warn">/ conflicting records</span>{/if}
            {#if timingLabel(revision, now)}<span class="warn">/ {timingLabel(revision, now)?.toLowerCase()}</span>{/if}
          </p>
          <a href={recordHref(record.id, record.node, recordsHref(new URLSearchParams(params)))} style:display="block" style:margin-top="3px" style:font-size="15px" style:overflow-wrap="anywhere" style:white-space="pre-wrap">{revision.statement}</a>
          <p class="meta" style:margin-top="4px" style:overflow-wrap="anywhere">{scopeLabel(record.scope)} / {record.node || "this machine"} / {whenLabel(revision.recordedAt, now)}</p>
        </article>
      {/if}
    {/each}
  </section>
  {#if initial.has("offset") || initial.has("cursor_node") || hasNext}
    <nav aria-label="Record pages" class="hd">
      <span class="meta">{page.data.records.length} on this page</span>
      <span class="acts">
        <button class="btn" type="button" disabled={!(initial.has("offset") || initial.has("cursor_node"))} onclick={firstPage}>First page</button>
        <button class="btn" type="button" disabled={!hasNext} onclick={nextPage}>Next page</button>
      </span>
    </nav>
  {/if}
{/if}
