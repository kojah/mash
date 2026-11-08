<script lang="ts">
import { agents } from "../lib/agents";
import type { Agent } from "../lib/types";

let { agent, size = 16 }: { agent: Agent; size?: number } = $props();
const info = $derived(agents[agent]);
</script>

<span class="inline-flex shrink-0 items-center justify-center text-ink-2" style:width="{size + 2}px" style:height="{size + 2}px" aria-hidden="true">
  <img src={info.logo} alt="" width={size} height={size} class="block" />
</span>
