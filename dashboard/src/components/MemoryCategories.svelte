<script lang="ts">
import BadgeCheck from "@lucide/svelte/icons/badge-check";
import CircleHelp from "@lucide/svelte/icons/circle-help";
import Gavel from "@lucide/svelte/icons/gavel";
import SlidersHorizontal from "@lucide/svelte/icons/sliders-horizontal";
import { recordsHref } from "../lib/route.svelte";

let { vertical = false, onNavigate }: { vertical?: boolean; onNavigate?: () => void } = $props();
const categories = [
  { kind: "decision", label: "Decisions", icon: Gavel, color: "#4757ae" },
  { kind: "preference", label: "Preferences", icon: SlidersHorizontal, color: "#2c776b" },
  { kind: "fact", label: "Facts", icon: BadgeCheck, color: "#8f641e" },
  { kind: "open_question", label: "Questions", icon: CircleHelp, color: "#984878" },
] as const;
</script>

<div class:vertical class="memory-categories">
  {#each categories as category (category.kind)}
    <a href={recordsHref(new URLSearchParams({ kind: category.kind }))} onclick={onNavigate}>
      <category.icon size={15} strokeWidth={1.75} color={category.color} aria-hidden="true" />
      <span>{category.label}</span>
    </a>
  {/each}
</div>
