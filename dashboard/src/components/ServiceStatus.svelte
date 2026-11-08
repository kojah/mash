<script lang="ts">
import type { ServiceState } from "../lib/services.svelte";
import { stateLabel } from "../lib/services.svelte";

let { state, label }: { state: ServiceState; label?: string } = $props();
</script>
<span class="service-status"><span class="service-dot {state}" aria-hidden="true"></span><span>{label ?? stateLabel[state]}</span></span>
