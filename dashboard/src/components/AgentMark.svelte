<script lang="ts">
import { agents } from "../lib/agents";
import type { Agent } from "../lib/types";

// Provider marks are single-colour drawings; the stylesheet inverts them for
// the dark ground.
let { agent, size = 19 }: { agent: Agent; size?: number } = $props();
const info = $derived(agents[agent]);
</script>

<img class="mark" src={info.logo} alt="" width={size} height={size} style:width="{size}px" style:height="{size}px" />
