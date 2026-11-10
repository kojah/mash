<script lang="ts">
import { agents } from "../lib/agents";
import type { Agent } from "../lib/types";

// Provider marks are single-colour drawings. Each is used as a mask over its
// agent's colour, so agents can be told apart at a glance.
let { agent, size = 19 }: { agent: Agent; size?: number } = $props();
const info = $derived(agents[agent]);
</script>

<span class="mark tint" aria-hidden="true" style:width="{size}px" style:height="{size}px" style:background-color={info.tint} style:mask-image="url({info.logo})" style:-webkit-mask-image="url({info.logo})"></span>
