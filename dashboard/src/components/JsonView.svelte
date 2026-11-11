<script lang="ts">
import JsonView from "./JsonView.svelte";

// A reply as indented JSON. Long lists show their first entry and the reader
// opens the rest a page at a time, so a reply of thousands of rows stays light.
// Everything is rendered as text: replies carry archived, untrusted content.
let { value, depth = 0 }: { value: unknown; depth?: number } = $props();
const PAGE = 50;
const LONG = 600;
const isList = $derived(Array.isArray(value));
const entries = $derived(
  Array.isArray(value)
    ? value.map((item, index) => [String(index), item] as const)
    : value !== null && typeof value === "object"
      ? Object.entries(value)
      : null,
);
let limit = $state(1);
let whole = $state(false);
const folds = $derived(isList && (entries?.length ?? 0) > 3);
const shown = $derived(entries && folds ? entries.slice(0, limit) : (entries ?? []));
const rest = $derived((entries?.length ?? 0) - shown.length);
const tone = $derived(
  typeof value === "string" ? "s" : typeof value === "number" ? "n" : value === null ? "z" : "b",
);
const literal = $derived(entries ? "" : JSON.stringify(value));
</script>

{#if !entries}
  {#if literal.length > LONG && !whole}
    <span class="j-{tone}">{literal.slice(0, LONG)}…</span> <button class="j-more" type="button" onclick={() => (whole = true)}>show all {literal.length.toLocaleString()} characters</button>
  {:else}<span class="j-{tone}">{literal}</span>{/if}
{:else if entries.length === 0}
  <span>{isList ? "[]" : "{}"}</span>
{:else}
  <span>{isList ? "[" : "{"}</span>
  <div class="j-in">
    {#each shown as [key, item], index (key)}
      <div>{#if !isList}<span class="j-k">{JSON.stringify(key)}</span>{/if}<JsonView value={item} depth={depth + 1} />{#if index < shown.length - 1 || rest > 0},{/if}</div>
    {/each}
    {#if rest > 0}
      <button class="j-more" type="button" onclick={() => (limit += PAGE)}>show {Math.min(PAGE, rest).toLocaleString()} more of {rest.toLocaleString()}</button>
    {/if}
  </div>
  <span>{isList ? "]" : "}"}</span>
{/if}
