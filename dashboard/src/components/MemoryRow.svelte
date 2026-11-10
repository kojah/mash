<script lang="ts">
import BadgeCheck from "@lucide/svelte/icons/badge-check";
import CircleHelp from "@lucide/svelte/icons/circle-help";
import Gavel from "@lucide/svelte/icons/gavel";
import SlidersHorizontal from "@lucide/svelte/icons/sliders-horizontal";
import { whenLabel } from "../lib/browse";
import { label, type NativeRecord } from "../lib/records";
import { recordHref } from "../lib/route.svelte";

let { record, back = "#/records" }: { record: NativeRecord; back?: string } = $props();
const icons = {
  decision: Gavel,
  preference: SlidersHorizontal,
  fact: BadgeCheck,
  open_question: CircleHelp,
} as const;
const revision = $derived(
  record.revisions.find((revision) => revision.revision === record.currentRevision) ??
    record.revisions[0],
);
</script>

{#if revision}
  {@const Icon = icons[revision.details.kind]}
  <a class="mem" href={recordHref(record.id, record.node, back)}>
    <Icon size={18} aria-hidden="true" />
    <span style:min-width="0">
      <span class="clamp">{revision.statement}</span>
      <span class="meta" style:display="block" style:margin-top="2px">{label(revision.details.kind).toLowerCase()}{revision.review === "candidate" ? " / unconfirmed" : ""} / {whenLabel(revision.recordedAt, Date.now())}</span>
    </span>
  </a>
{/if}
