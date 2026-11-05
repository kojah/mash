<script lang="ts">
import Check from "@lucide/svelte/icons/check";
import Copy from "@lucide/svelte/icons/copy";

let { code, language, file }: { code: string; language: string; file?: string | undefined } =
  $props();
let copied = $state(false);

async function copy(): Promise<void> {
  await navigator.clipboard?.writeText(code);
  copied = true;
  setTimeout(() => {
    copied = false;
  }, 1500);
}
</script>

<!-- Syntax highlighting (Shiki, lazily loaded per language) comes with the real data. -->
<figure class="m-0 w-full overflow-hidden rounded-xl bg-code">
  <figcaption class="flex items-center gap-2 bg-code-bar py-1.5 pr-1.5 pl-3.5 font-mono text-xs text-[#a9b0bc]">
    <span class="flex-1">{file ? `${file} · ` : ""}{language}</span>
    <button type="button" aria-label={copied ? "Copied" : "Copy code"} onclick={copy} class="inline-flex size-9 items-center justify-center rounded-lg text-[#a9b0bc]">
      {#if copied}<Check size={16} strokeWidth={2} />{:else}<Copy size={16} strokeWidth={1.75} />{/if}
    </button>
  </figcaption>
  <pre class="m-0 overflow-x-auto px-4 py-3.5 font-mono text-[13px] leading-relaxed text-code-ink"><code>{code}</code></pre>
</figure>
