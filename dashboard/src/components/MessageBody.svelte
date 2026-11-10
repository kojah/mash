<script lang="ts">
import { markdown } from "../lib/markdown";

let { text }: { text: string } = $props();
let expanded = $state(false);
</script>

<div class="prose min-w-0 break-words" class:collapsed={!expanded && text.length > 16_384}>{@html markdown(text)}</div>
{#if text.length > 16_384}
  <button type="button" class="a" style:align-self="flex-start" style:font-size="12.5px" onclick={() => expanded = !expanded}>{expanded ? "Collapse" : `Show full message (${text.length.toLocaleString()} characters)`}</button>
{/if}

<style>
.collapsed { max-height: 32rem; overflow: hidden; mask-image: linear-gradient(black 90%, transparent); }
.prose :global(p) { margin: 0 0 .75rem; }
.prose :global(h1), .prose :global(h2), .prose :global(h3), .prose :global(h4) { font-weight: 650; line-height: 1.3; margin: 1.25rem 0 .65rem; }
.prose :global(h1) { font-size: 16px; }
.prose :global(h2) { font-size: 15px; }
.prose :global(h3) { font-size: 14px; }
.prose :global(ul) { list-style: disc; padding-left: 1.5rem; margin-bottom: .8rem; }
.prose :global(ol) { list-style: decimal; padding-left: 1.5rem; margin-bottom: .8rem; }
.prose :global(li) { margin: .3rem 0; }
.prose :global(blockquote) { border-left: 3px solid var(--color-line); padding-left: 1rem; color: var(--color-muted); }
.prose :global(pre) { white-space: pre; overflow-x: auto; padding: 12px; background: var(--raised); border-radius: 6px; font-size: 12px; }
.prose :global(code) { font-size: 12.5px; }
.prose :global(:not(pre) > code) { padding: 1px 5px; border-radius: 4px; background: var(--soft); }
.prose :global(p:last-child) { margin-bottom: 0; }
.prose :global(a) { color: var(--clay2); text-decoration: underline; text-underline-offset: 3px; }
.prose :global(table) { display: block; overflow-x: auto; border-collapse: collapse; }
.prose :global(td), .prose :global(th) { padding: .4rem .6rem; border-bottom: 1px solid var(--color-line); text-align: left; }
</style>
