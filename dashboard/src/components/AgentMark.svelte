<script lang="ts">
import Bot from "@lucide/svelte/icons/bot";
import Code from "@lucide/svelte/icons/code";
import MousePointer2 from "@lucide/svelte/icons/mouse-pointer-2";
import { agents } from "../lib/agents";
import type { Agent } from "../lib/types";

let { agent, size = 16 }: { agent: Agent; size?: number } = $props();
const info = $derived(agents[agent]);
</script>

<span class="inline-flex shrink-0 items-center justify-center text-ink-2" style:width="{size + 2}px" style:height="{size + 2}px" aria-hidden="true">
  {#if info.logo}
    <img src={info.logo} alt="" width={size} height={size} class="block" />
  {:else if agent === "cursor"}
    <MousePointer2 {size} strokeWidth={1.75} />
  {:else if agent === "opencode"}
    <Code {size} strokeWidth={1.75} />
  {:else}
    <Bot {size} strokeWidth={1.75} />
  {/if}
</span>
