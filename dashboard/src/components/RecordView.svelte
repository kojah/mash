<script lang="ts">
import Clock from "@lucide/svelte/icons/clock";
import FileText from "@lucide/svelte/icons/file-text";
import Link2 from "@lucide/svelte/icons/link-2";
import { createQuery } from "@tanstack/svelte-query";
import { stableRead } from "../lib/queries";
import { type Evidence, getRecord, label, scopeLabel, standing, timingLabel } from "../lib/records";
import { chatHref, recordHref } from "../lib/route.svelte";
import LoadError from "./LoadError.svelte";
import Skeleton from "./Skeleton.svelte";

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

{#snippet sources(evidence: readonly Evidence[], node: string)}
  {#each evidence as source, index (index)}
    <figure style:padding="12px 14px" style:border-radius="6px" style:background="var(--raised)" style:margin-top={index ? "8px" : "0"}>
      <blockquote style:border="0" style:padding="0" style:white-space="pre-wrap" style:overflow-wrap="anywhere">{source.quote}</blockquote>
      <figcaption>
        <a href={chatHref(source.chatId, node, source.messageId, Boolean(node))} style:display="flex" style:flex-wrap="wrap" style:align-items="center" style:gap="2px 10px" style:min-height="44px" style:font-size="12px">
          <span class="a">{source.sourceRole === "user" ? "you said this" : `${source.sourceRole || "someone"} said this`}{source.purpose ? ` (${label(source.purpose).toLowerCase()})` : ""}</span><span>open the message</span>
        </a>
      </figcaption>
    </figure>
  {/each}
{/snippet}

<a class="back" href={back}>&lt; memory</a>
{#if latest.isPending}<Skeleton rows={4} />{/if}
{#if latest.error}<LoadError title="Couldn’t load this memory" message={latest.error.message} retry={refresh} busy={latest.isFetching}/>{/if}
{#if page.error && !latest.error}<LoadError title="Couldn’t load memory history" message={page.error.message} retry={() => page.refetch()} busy={page.isFetching} retryLabel="Retry history"/>{/if}
{#if record && current && !latest.error && !page.error}
  <header>
    <p style:color="var(--pink)" style:font-size="12.5px">{label(current.details.kind).toLowerCase()} / {label(current.details.state).toLowerCase()} / {standing(current)}</p>
    <h1 class="h1" style:margin-top="6px" style:font-weight="500" style:white-space="pre-wrap" style:overflow-wrap="anywhere">{current.statement}</h1>
    <p class="meta" style:margin-top="8px" style:overflow-wrap="anywhere">{scopeLabel(record.scope)} / {record.node || "this machine"} / recorded by {current.recordedBy} / {date(current.recordedAt)}</p>
    {#if timingLabel(current, Date.now())}<p class="warn" role="status" style:margin-top="8px">{timingLabel(current, Date.now())}. Check the effective and expiry dates before relying on this.</p>{/if}
    {#if record.conflicted}<p class="warn" role="status" style:margin-top="8px">Another record contradicts this one. Look at the related records before relying on it.</p>{/if}
    {#if !current.evidenceCurrent}<p class="warn" role="status" style:margin-top="8px">The quoted messages have changed or are missing. Review the sources before relying on this.</p>{/if}
  </header>
  {#if current.rationale}<section><h2 class="h2">why</h2><p style:white-space="pre-wrap" style:overflow-wrap="anywhere">{current.rationale}</p></section>{/if}
  <section>
    <h2 class="h2"><FileText size={15} aria-hidden="true" />evidence{current.evidenceCurrent ? "" : " (stale or missing)"}</h2>
    {@render sources(current.evidence, record.node)}
  </section>
  <section>
    <div class="hd"><h2 class="h2"><Clock size={15} aria-hidden="true" />revision history</h2><button class="a meta" type="button" onclick={refresh} disabled={latest.isFetching} style:min-height="44px">check for updates</button></div>
    {#if page.isFetching}<p class="meta" role="status">Loading history…</p>{/if}
    {#if page.data}
      {#if !page.data.revisions.length}<p class="meta">No revisions on this page.</p>{/if}
      {#each page.data.revisions as revision (revision.revision)}
        <div class="row">
          <span style:display="block">Revision {revision.revision}{revision.revision === record.currentRevision ? " (current)" : ""}: {label(revision.details.state).toLowerCase()}, {standing(revision)}</span>
          <span class="meta">{date(revision.recordedAt)} / {revision.recordedBy}{revision.effectiveAt === null ? "" : ` / applies from ${date(revision.effectiveAt)}`}</span>
          {#if revision.revision !== record.currentRevision}
            <p style:margin-top="6px" style:white-space="pre-wrap" style:overflow-wrap="anywhere">{revision.statement}</p>
            {#if revision.rationale}<p class="meta" style:white-space="pre-wrap" style:overflow-wrap="anywhere">{revision.rationale}</p>{/if}
          {/if}
          {#if revision.details.kind === "decision"}
            {#if revision.details.alternatives.length}<p class="meta" style:margin-top="6px">alternatives</p><ul class="mdl">{#each revision.details.alternatives as item, index (index)}<li style:overflow-wrap="anywhere">{item}</li>{/each}</ul>{/if}
            {#if revision.details.consequences.length}<p class="meta" style:margin-top="6px">consequences</p><ul class="mdl">{#each revision.details.consequences as item, index (index)}<li style:overflow-wrap="anywhere">{item}</li>{/each}</ul>{/if}
          {:else if revision.details.kind === "preference"}
            {#if revision.details.conditions.length}<p class="meta" style:margin-top="6px">conditions</p><ul class="mdl">{#each revision.details.conditions as item, index (index)}<li style:overflow-wrap="anywhere">{item}</li>{/each}</ul>{/if}
          {:else if revision.details.kind === "fact"}
            <p class="meta" style:margin-top="6px">reported by {revision.details.reportedBy}{revision.details.validUntil === null ? "" : ` / valid until ${date(revision.details.validUntil)}`}</p>
          {:else if revision.details.answer}
            <p class="meta" style:margin-top="6px">answer</p><p style:white-space="pre-wrap" style:overflow-wrap="anywhere">{revision.details.answer}</p>
          {/if}
          {#if revision.revision !== record.currentRevision}
            <p class="meta" style:margin="8px 0 6px">evidence{revision.evidenceCurrent ? "" : " (stale or missing)"}</p>
            {@render sources(revision.evidence, record.node)}
          {/if}
        </div>
      {/each}
      {#if before !== null || page.data.nextBeforeRevision !== null}
        <nav aria-label="Revision pages" class="acts" style:margin-top="10px">
          {#if before !== null}<button class="btn" type="button" onclick={() => (before = null)}>Newest revisions</button>{/if}
          {#if page.data.nextBeforeRevision !== null}<button class="btn" type="button" onclick={() => (before = page.data?.nextBeforeRevision ?? null)}>Older revisions</button>{/if}
        </nav>
      {/if}
    {/if}
  </section>
  {#if page.data && (page.data.links.length || linkOffset > 0)}
    <section>
      <h2 class="h2"><Link2 size={15} aria-hidden="true" />related records</h2>
      {#if !page.data.links.length}<p class="meta">No relationships on this page.</p>{/if}
      {#each page.data.links as link (JSON.stringify(link))}
        {@const outgoing = link.sourceId === record.id}
        {@const related = outgoing ? link.targetId : link.sourceId}
        <a class="li" href={recordHref(related, record.node, back)}><span class="g" style:overflow-wrap="anywhere">{outgoing ? `this record ${link.relation}` : `${link.relation} this record`}</span><span class="n" style:overflow="hidden" style:text-overflow="ellipsis" style:max-width="50%">{related.slice(-12)}</span></a>
      {/each}
      {#if linkOffset > 0 || page.data.nextLinkOffset !== null}
        <nav aria-label="Relationship pages" class="acts" style:margin-top="10px">
          {#if linkOffset > 0}<button class="btn" type="button" onclick={() => (linkOffset = 0)}>First relationships</button>{/if}
          {#if page.data.nextLinkOffset !== null}<button class="btn" type="button" onclick={() => (linkOffset = page.data?.nextLinkOffset ?? 0)}>More relationships</button>{/if}
        </nav>
      {/if}
    </section>
  {/if}
  <p class="meta" style:overflow-wrap="anywhere">record {record.id} / owner {record.owner} / reading a record does not confirm it</p>
{/if}
