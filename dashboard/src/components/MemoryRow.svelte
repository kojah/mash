<script lang="ts">
import { whenLabel } from "../lib/browse";
import { kindIcons } from "../lib/recordIcons";
import { label, type NativeRecord, standing } from "../lib/records";
import { recordHref } from "../lib/route.svelte";

let { record, back = "#/records" }: { record: NativeRecord; back?: string } = $props();
const revision = $derived(
  record.revisions.find((revision) => revision.revision === record.currentRevision) ??
    record.revisions[0],
);
</script>

{#if revision}
  {@const Icon = kindIcons[revision.details.kind]}
  <a class="mem" href={recordHref(record.id, record.node, back)}>
    <Icon size={18} aria-hidden="true" />
    <span style:min-width="0">
      <span class="clamp">{revision.statement}</span>
      <span class="meta" style:display="block" style:margin-top="2px">{label(revision.details.kind).toLowerCase()}{standing(revision) === "established" ? "" : ` / ${standing(revision)}`} / {whenLabel(revision.recordedAt, Date.now())}</span>
    </span>
  </a>
{/if}
