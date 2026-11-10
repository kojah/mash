<script lang="ts">
import Check from "@lucide/svelte/icons/check";
import ChevronDown from "@lucide/svelte/icons/chevron-down";
import type { Snippet } from "svelte";

// A filter chip that opens a menu of choices. `draft` marks a choice that
// has been changed on the page but not yet applied to the results.
let {
  label,
  icon,
  active = false,
  draft = false,
  multiple = false,
  options,
  selected,
  choose,
}: {
  label: string;
  icon: Snippet;
  active?: boolean;
  draft?: boolean;
  multiple?: boolean;
  options: readonly { value: string; label: string; count?: number }[];
  selected: readonly string[];
  choose: (value: string) => void;
} = $props();
let open = $state(false);
let root: HTMLDivElement;
</script>

<svelte:window
  onclick={(event) => {
    if (open && event.target instanceof Node && !root.contains(event.target)) open = false;
  }}
  onkeydown={(event) => {
    if (event.key === "Escape") open = false;
  }}
/>
<div class="fwrap" bind:this={root}>
  <button class="chip" class:on={active} class:draft type="button" aria-haspopup="true" aria-expanded={open} onclick={() => (open = !open)}>
    {@render icon()}{label}<ChevronDown size={12} color="var(--dim)" aria-hidden="true" />
  </button>
  {#if open}
    <div class="fmenu" role="group" aria-label={label}>
      {#each options as option (option.value)}
        {@const on = selected.includes(option.value)}
        <button
          type="button"
          role={multiple ? "menuitemcheckbox" : "menuitemradio"}
          aria-checked={on}
          onclick={() => {
            choose(option.value);
            if (!multiple) open = false;
          }}
        >
          <span style:flex="none" style:width="16px" style:display="inline-flex">{#if on}<Check size={14} color="var(--clay2)" aria-hidden="true" />{/if}</span>
          <span class="g">{option.label}</span>
          {#if option.count !== undefined}<span class="n">{option.count.toLocaleString()}</span>{/if}
        </button>
      {:else}
        <p class="meta" style:padding="8px 9px">Nothing to choose from yet.</p>
      {/each}
    </div>
  {/if}
</div>
