<script lang="ts">
import { markdown } from "../lib/markdown";

let { text }: { text: string } = $props();
let expanded = $state(false);
const visible = $derived(expanded ? text : text.slice(0, 16_384));
</script>

<div class="prose min-w-0 break-words">{@html markdown(visible)}</div>
{#if text.length > 16_384}
  <button type="button" class="text-sm text-accent" onclick={() => expanded = !expanded}>{expanded ? "Collapse" : `Show full message (${text.length.toLocaleString()} characters)`}</button>
{/if}

<style>
.prose :global(p) { margin: 0 0 .75rem; }
.prose :global(pre) { white-space: pre; overflow-x: auto; padding: .8rem; background: var(--color-fill); border-radius: .5rem; }
.prose :global(code) { font-size: .85em; }
.prose :global(a) { color: var(--color-accent); text-decoration: underline; }
.prose :global(table) { display: block; overflow-x: auto; border-collapse: collapse; }
.prose :global(td), .prose :global(th) { padding: .4rem; border: 1px solid var(--color-line); }
</style>
