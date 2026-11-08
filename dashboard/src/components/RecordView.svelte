<script lang="ts">
import { createQuery } from "@tanstack/svelte-query";
import { stableRead } from "../lib/queries";
import { getRecord, label, scopeLabel, timingLabel } from "../lib/records";
import { chatHref, recordHref } from "../lib/route.svelte";

let { id, node, back }: { id: string; node: string; back: string } = $props();
let before = $state<number | null>(null);
let linkOffset = $state(0);
const latest = createQuery(() => ({
  ...stableRead,
  queryKey: ["record", id, node, null, 0],
  queryFn: ({ signal }) => getRecord(id, node, null, 0, signal),
}));
const history = createQuery(() => ({
  ...stableRead,
  queryKey: ["record-history", id, node, latest.dataUpdatedAt, before, linkOffset],
  enabled: before !== null || linkOffset !== 0,
  queryFn: ({ signal }) => getRecord(id, node, before, linkOffset, signal),
}));
const page = $derived(before === null && linkOffset === 0 ? latest : history);
const record = $derived(latest.data);
const current = $derived(
  record?.revisions.find((revision) => revision.revision === record.currentRevision),
);
async function refresh(): Promise<void> {
  before = null;
  linkOffset = 0;
  await latest.refetch();
}
function date(value: number | null): string {
  return value === null ? "Unknown" : new Date(value).toLocaleString();
}
</script>

<main class="page-column memory-detail flex flex-col gap-6">
  <nav class="flex flex-wrap gap-5 text-sm"><a href="#/">Chats</a><a href={back}>Back to memory</a><button type="button" onclick={refresh} disabled={latest.isFetching}>Refresh record</button></nav>
  {#if latest.isPending}<p role="status">Loading record…</p>{/if}
  {#if latest.error}<p role="alert">{latest.error.message} <button type="button" onclick={refresh}>Retry</button></p>{/if}
  {#if page.error && !latest.error}<p role="alert">{page.error.message} <button type="button" onclick={() => page.refetch()}>Retry history</button></p>{/if}
  {#if record && current && !latest.error && !page.error}
    <header>
      <p class="m-0 text-sm text-muted">{label(current.details.kind)} · {label(current.details.state)} · {label(current.review)}</p>
      {#if timingLabel(current, Date.now())}<p role="status">{timingLabel(current, Date.now())}. Check the effective and expiry dates before relying on this record.</p>{/if}
      <h1 class="my-3 whitespace-pre-wrap break-words text-3xl font-semibold">{current.statement}</h1>
      <p class="text-sm text-muted">{scopeLabel(record.scope)} · {record.node || "Local"} · Current revision {record.currentRevision}</p>
      <details class="text-xs text-muted"><summary>Record details</summary><p class="break-all">Record {record.id} · Owner {record.owner}</p><p>Read-only. Reading does not confirm a record.</p></details>
      {#if record.conflicted}<p role="status" class="rounded-xl border border-line p-3">This record has a declared conflict. Inspect the related records before relying on it.</p>{/if}
      {#if !current.evidenceCurrent}<p role="status" class="rounded-xl border border-line p-3">The supporting evidence has changed or is missing. Review the sources before relying on this record.</p>{/if}
    </header>
    <section aria-labelledby="evidence-heading" class="flex flex-col gap-3">
      <h2 id="evidence-heading" class="section-heading">Supporting messages{current.evidenceCurrent ? "" : " · Stale or missing"}</h2>
      {#each current.evidence as source, index (index)}
        <blockquote class="mx-0 border-l-2 border-accent pl-4">
          <p class="whitespace-pre-wrap break-words text-sm">{source.quote}</p>
          <p class="text-xs text-muted">{source.sourceRole || "Role unknown"} · {source.purpose ? label(source.purpose) : "Purpose unknown"}</p>
          <a class="break-all text-sm" href={chatHref(source.chatId, record.node, source.messageId, Boolean(record.node))}>Open source message</a>
        </blockquote>
      {/each}
    </section>
    <section aria-labelledby="history-heading" class="flex flex-col gap-4">
      <h2 id="history-heading" class="m-0 text-lg font-semibold">Revision history</h2>
      {#if page.isFetching}<p role="status">Loading history…</p>{/if}
      {#if page.data}
        {#if !page.data.revisions.length}<p>No revisions remain on this page. Refresh to inspect the current record.</p>{/if}
        {#each page.data.revisions as revision (revision.revision)}
          <article class="border-l border-line pl-5">
            <h3 class="m-0 font-medium">Revision {revision.revision}{revision.revision === record.currentRevision ? " · Current" : ""}</h3>
            <p class="text-xs text-muted">{date(revision.recordedAt)} · {label(revision.details.state)} · {label(revision.review)}</p>
            <p class="whitespace-pre-wrap break-words font-medium">{revision.statement}</p>
            {#if revision.rationale}<p class="whitespace-pre-wrap break-words text-sm">{revision.rationale}</p>{/if}
            <p class="text-xs text-muted">Recorded by {revision.recordedBy} · Applies from {date(revision.effectiveAt)}</p>
            {#if revision.details.kind === "decision"}
              {#if revision.details.alternatives.length}<h4 class="text-sm font-medium">Alternatives</h4><ul>{#each revision.details.alternatives as item, index (index)}<li class="whitespace-pre-wrap break-words text-sm">{item}</li>{/each}</ul>{/if}
              {#if revision.details.consequences.length}<h4 class="text-sm font-medium">Consequences</h4><ul>{#each revision.details.consequences as item, index (index)}<li class="whitespace-pre-wrap break-words text-sm">{item}</li>{/each}</ul>{/if}
            {:else if revision.details.kind === "preference"}
              {#if revision.details.conditions.length}<h4 class="text-sm font-medium">Conditions</h4><ul>{#each revision.details.conditions as item, index (index)}<li class="whitespace-pre-wrap break-words text-sm">{item}</li>{/each}</ul>{/if}
            {:else if revision.details.kind === "fact"}
              <p class="text-sm">Reported by {revision.details.reportedBy} · Valid until {date(revision.details.validUntil)}</p>
            {:else if revision.details.answer}
              <h4 class="text-sm font-medium">Answer</h4><p class="whitespace-pre-wrap break-words text-sm">{revision.details.answer}</p>
            {/if}
            {#if revision.revision !== record.currentRevision}
            <h4 class="text-sm font-medium">Supporting messages{revision.evidenceCurrent ? "" : " · Stale or missing"}</h4>
            {#each revision.evidence as source, index (index)}
              <blockquote class="mx-0 border-l-2 border-line pl-4">
                <p class="whitespace-pre-wrap break-words text-sm">{source.quote}</p>
                <p class="text-xs text-muted">{source.sourceRole || "Role unknown"} · {source.purpose ? label(source.purpose) : "Purpose unknown"}</p>
                <a class="break-all text-sm" href={chatHref(source.chatId, record.node, source.messageId, Boolean(record.node))}>Open source message</a>
              </blockquote>
            {/each}
            {/if}
          </article>
        {/each}
        <nav aria-label="Revision pages" class="flex gap-5 text-sm">{#if before !== null}<button type="button" onclick={() => before = null}>Newest revisions</button>{/if}{#if page.data.nextBeforeRevision !== null}<button type="button" onclick={() => before = page.data?.nextBeforeRevision ?? null}>Older revisions</button>{/if}</nav>
      {/if}
    </section>
    <section aria-labelledby="links-heading" class="border-t border-line pt-5">
      <h2 id="links-heading" class="m-0 text-lg font-semibold">Related records</h2>
      {#if page.data}
        {#if !page.data.links.length}<p class="text-sm text-muted">No relationships on this page.</p>{/if}
        <ul class="list-none space-y-3 p-0">{#each page.data.links as link (JSON.stringify(link))}
          {@const outgoing = link.sourceId === record.id}
          {@const related = outgoing ? link.targetId : link.sourceId}
          <li class="break-all text-sm"><span class="text-muted">{outgoing ? "This record" : "Related record"} {link.relation} {outgoing ? "related record" : "this record"}: </span><a href={recordHref(related, record.node, back)}>{related}</a></li>
        {/each}</ul>
        <nav aria-label="Relationship pages" class="flex gap-5 text-sm">{#if linkOffset > 0}<button type="button" onclick={() => linkOffset = 0}>First relationships</button>{/if}{#if page.data.nextLinkOffset !== null}<button type="button" onclick={() => linkOffset = page.data?.nextLinkOffset ?? 0}>More relationships</button>{/if}</nav>
      {/if}
    </section>
  {/if}
</main>
