<script lang="ts">
import { createQuery } from "@tanstack/svelte-query";
import { untrack } from "svelte";
import { catalogOptions, stableRead } from "../lib/queries";
import { kinds, label, recordPage, scopeLabel, states, timingLabel } from "../lib/records";
import { recordHref, recordsHref } from "../lib/route.svelte";

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
  location.hash = recordsHref(next);
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
  location.hash = recordsHref(next);
}
function firstPage(): void {
  const next = new URLSearchParams(params);
  next.delete("offset");
  next.delete("cursor_node");
  next.delete("cursor_offset");
  location.hash = recordsHref(next);
}
</script>

<main class="mx-auto flex max-w-[1000px] flex-col gap-6 px-5 py-10">
  <nav class="flex items-center gap-5 text-sm"><a href="#/">Chats</a><span aria-current="page" class="font-semibold">Records</span></nav>
  <header><h1 class="m-0 text-3xl font-semibold">Records</h1><p class="mt-2 text-sm text-muted">Decisions, preferences, facts and open questions, with the conversations behind them.</p></header>
  <form onsubmit={submit} class="flex flex-wrap items-end gap-3 rounded-xl bg-surface p-4">
    <label class="flex min-w-48 flex-1 flex-col gap-1 text-xs text-muted">Search statements<input type="search" bind:value={query} class="rounded-lg border border-line bg-ground p-2 text-sm text-ink" placeholder="Find a conclusion" /></label>
    <label class="flex flex-col gap-1 text-xs text-muted">Kind<select bind:value={kind} class="rounded-lg border border-line bg-ground p-2 text-sm text-ink"><option value="">All kinds</option>{#each kinds as value (value)}<option value={value}>{label(value)}</option>{/each}</select></label>
    <label class="flex flex-col gap-1 text-xs text-muted">Status<select bind:value={lifecycle} class="rounded-lg border border-line bg-ground p-2 text-sm text-ink"><option value="">All statuses</option>{#each states as value (value)}<option value={value}>{label(value)}</option>{/each}</select></label>
    <label class="flex flex-col gap-1 text-xs text-muted">Review<select bind:value={review} class="rounded-lg border border-line bg-ground p-2 text-sm text-ink"><option value="">All reviews</option><option value="established">Established</option><option value="candidate">Candidate</option></select></label>
    <label class="flex flex-col gap-1 text-xs text-muted">Source<select bind:value={source} class="max-w-44 rounded-lg border border-line bg-ground p-2 text-sm text-ink"><option value="">Local records</option><option value="shared">All shared nodes</option>{#each nodes as node (node)}<option value={`node:${node}`}>{node} (shared)</option>{/each}</select></label>
    <button type="submit" class="rounded-lg bg-ink px-4 py-2 text-sm text-ground">Apply filters</button>
  </form>
  {#if chat}<p class="m-0 text-sm text-muted">Records citing chat <span class="break-all font-mono">{chat}</span> in any retained revision. <a href="#/records">Show all records</a></p>{/if}
  <div class="flex justify-between gap-4 text-xs text-muted"><span>Read-only · Includes candidates and resolved or retired records.</span><button type="button" onclick={() => page.refetch()} disabled={page.isFetching}>Refresh records</button></div>
  {#if page.isPending}<p role="status">Loading records…</p>{/if}
  {#if page.error}<p role="alert">{page.error.message} <button type="button" onclick={() => page.refetch()}>Retry</button></p>{/if}
  {#if page.data && !page.error}
    {#if !page.data.records.length}<p>{hasNext ? "No matching records on this page. Continue to the next page." : "No records match these filters. Records are saved through your agent’s Mash tools."}</p>{/if}
    <ul class="m-0 flex list-none flex-col gap-3 p-0">
      {#each page.data.records as record (JSON.stringify([record.node, record.owner, record.id]))}
        {@const revision = record.revisions.find(revision => revision.revision === record.currentRevision)}
        {#if revision}
          <li><a href={recordHref(record.id, record.node, recordsHref(new URLSearchParams(params)))} class="block rounded-xl border border-line bg-surface p-5 hover:bg-fill">
            <div class="flex flex-wrap gap-2 text-xs text-muted"><span>{label(revision.details.kind)}</span><span>· {label(revision.details.state)}</span><span>· {label(revision.review)}</span>{#if !revision.evidenceCurrent}<strong class="text-ink">· Stale evidence</strong>{/if}{#if record.conflicted}<strong class="text-ink">· Conflicting records</strong>{/if}</div>
            {#if timingLabel(revision, Date.now())}<p class="text-xs font-medium">{timingLabel(revision, Date.now())}</p>{/if}
            <h2 class="my-2 whitespace-pre-wrap break-words text-lg font-medium">{revision.statement}</h2>
            <p class="m-0 break-all text-xs text-muted">{scopeLabel(record.scope)} · {record.node || "Local"} · Revision {record.currentRevision} · {revision.confirmations} confirmations</p>
          </a></li>
        {/if}
      {/each}
    </ul>
    <nav aria-label="Record pages" class="flex gap-5 text-sm">{#if initial.has("offset") || initial.has("cursor_node")}<button type="button" onclick={firstPage}>First page</button>{/if}{#if hasNext}<button type="button" onclick={nextPage}>Next page</button>{/if}</nav>
  {/if}
</main>
