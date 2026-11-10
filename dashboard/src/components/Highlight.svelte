<script lang="ts">
// Marks the words of a query inside archived text. The text is rendered as
// text, never as markup.
let { text, query }: { text: string; query: string } = $props();
const parts = $derived.by(() => {
  const terms = query
    .split(/\s+/)
    .filter((term) => term.length > 1)
    .map((term) => term.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"));
  if (!terms.length) return [{ text, hit: false }];
  const pattern = new RegExp(`(${terms.join("|")})`, "gi");
  return text
    .split(pattern)
    .filter(Boolean)
    .map((part, index) => ({ text: part, hit: index % 2 === 1 }));
});
</script>

{#each parts as part, index (index)}{#if part.hit}<mark>{part.text}</mark>{:else}{part.text}{/if}{/each}
